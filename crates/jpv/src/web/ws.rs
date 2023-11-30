use std::borrow::Cow;
use std::net::SocketAddr;

use anyhow::Result;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::ConnectInfo;
use axum::response::IntoResponse;
use axum::Extension;
use futures::sink::SinkExt;
use futures::stream::SplitSink;
use futures::stream::StreamExt;
use lib::api;
use rand::prelude::*;
use rand::rngs::SmallRng;
use tokio::sync::broadcast::Receiver;
use tokio::time::Duration;
use tracing::{Instrument, Level};

use crate::system;

pub(super) async fn entry(
    ws: WebSocketUpgrade,
    Extension(system_events): Extension<system::SystemEvents>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    let receiver = system_events.0.subscribe();

    ws.on_upgrade(move |socket| async move {
        let span = tracing::span!(Level::INFO, "websocket", ?remote);

        if let Err(error) = run(receiver, socket).instrument(span).await {
            tracing::error!(?error);
        }
    })
}

fn decode_escaped(data: &[u8]) -> Option<String> {
    fn h(b: u8) -> Option<u32> {
        let b = match b {
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            b'0'..=b'9' => b - b'0',
            _ => return None,
        };

        Some(b as u32)
    }

    let mut s = String::new();

    let mut it = data.iter().copied();

    while let Some(b) = it.next() {
        match (b, it.clone().next()) {
            (b'\\', Some(b'u')) => {
                it.next();
                let [a, b, c, d] = [it.next()?, it.next()?, it.next()?, it.next()?];
                let [a, b, c, d] = [h(a)?, h(b)?, h(c)?, h(d)?];
                let c = a << 12 | b << 8 | c << 4 | d;
                s.push(char::from_u32(c)?);
            }
            (b'\\', Some(b'\\')) => {
                it.next();
                s.push('\\');
            }
            (c, _) if c.is_ascii() => {
                s.push(c as char);
            }
            _ => {}
        }
    }

    Some(s)
}

#[cfg(feature = "tesseract")]
fn trim_whitespace(input: &str) -> Cow<'_, str> {
    let mut output = String::new();
    let mut c = input.char_indices();

    'ws: {
        for (n, c) in c.by_ref() {
            if c.is_whitespace() {
                output.push_str(&input[..n]);
                break 'ws;
            }
        }

        return Cow::Borrowed(input);
    };

    for (_, c) in c {
        if !c.is_whitespace() {
            output.push(c);
        }
    }

    Cow::Owned(output)
}

async fn system_event(
    sink: &mut SplitSink<WebSocket, Message>,
    event: system::Event,
) -> Result<()> {
    match event {
        system::Event::SendClipboardData(clipboard) => match clipboard.mimetype.as_str() {
            "UTF8_STRING" | "text/plain;charset=utf-8" => {
                let event = api::ClientEvent::SendClipboardData(api::SendClipboard {
                    ty: Some("text/plain".to_owned()),
                    data: clipboard.data,
                });

                let json = serde_json::to_vec(&event)?;
                sink.send(Message::Binary(json)).await?;
            }
            "STRING" | "text/plain" => {
                let Some(data) = decode_escaped(&clipboard.data[..]) else {
                    tracing::warn!("failed to decode");
                    return Ok(());
                };

                let event = api::ClientEvent::SendClipboardData(api::SendClipboard {
                    ty: Some("text/plain".to_owned()),
                    data: data.into_bytes(),
                });

                let json = serde_json::to_vec(&event)?;
                sink.send(Message::Binary(json)).await?;
            }
            ty @ "application/json" => {
                let event = api::ClientEvent::SendClipboardData(api::SendClipboard {
                    ty: Some(ty.to_owned()),
                    data: clipboard.data,
                });

                let json = serde_json::to_vec(&event)?;
                sink.send(Message::Binary(json)).await?;
            }
            ty => {
                let Some(event) = handle_image(ty, &clipboard)? else {
                    return Ok(());
                };

                let json = serde_json::to_vec(&event)?;
                sink.send(Message::Binary(json)).await?;
            }
        },
    }

    Ok(())
}

#[cfg(not(feature = "tesseract"))]
fn handle_image(_: &str, _: &system::SendClipboardData) -> Result<Option<api::ClientEvent>> {
    Ok(None)
}

#[cfg(feature = "tesseract")]
fn handle_image(ty: &str, c: &system::SendClipboardData) -> Result<Option<api::ClientEvent>> {
    use image::ImageFormat;

    let format = match ty {
        "image/png" => ImageFormat::Png,
        "image/tiff" => ImageFormat::Tiff,
        "image/webp" => ImageFormat::WebP,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        _ => return Ok(None),
    };

    tracing::info!(len = c.data.len(), "Decoding image");

    let image = match image::load_from_memory_with_format(&c.data[..], format) {
        Ok(image) => image,
        Err(error) => {
            tracing::warn!(?error, "Failed to load clipboard image");
            return Ok(None);
        }
    };

    let data = image.as_bytes();
    let width = usize::try_from(image.width())?;
    let height = usize::try_from(image.height())?;
    let bytes_per_pixel = usize::try_from(image.color().bytes_per_pixel())?;

    tracing::info!(len = data.len(), width, height, bytes_per_pixel);

    let text = match tesseract::image_to_text("jpn", data, width, height, bytes_per_pixel) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(?error, "Image recognition failed");
            return Ok(None);
        }
    };

    let trimmed = trim_whitespace(&text[..]);

    tracing::info!(text = &text[..], ?trimmed, "Recognized");

    Ok(Some(api::ClientEvent::SendClipboardData(
        api::SendClipboard {
            ty: Some("text/plain".to_owned()),
            data: trimmed.into_owned().into_bytes(),
        },
    )))
}

async fn run(mut system_events: Receiver<system::Event>, socket: WebSocket) -> Result<()> {
    tracing::info!("Accepted");

    const CLOSE_NORMAL: u16 = 1000;
    const CLOSE_PROTOCOL_ERROR: u16 = 1002;
    const CLOSE_TIMEOUT: Duration = Duration::from_secs(30);
    const PING_TIMEOUT: Duration = Duration::from_secs(10);

    let (mut sender, mut receiver) = socket.split();

    let mut last_ping = None::<u32>;
    let mut rng = SmallRng::seed_from_u64(0x404241112);
    let mut close_interval = tokio::time::interval(CLOSE_TIMEOUT);
    close_interval.reset();

    let mut ping_interval = tokio::time::interval(PING_TIMEOUT);
    ping_interval.reset();

    let close_here = loop {
        tokio::select! {
            _ = close_interval.tick() => {
                break Some((CLOSE_NORMAL, "connection timed out"));
            }
            _ = ping_interval.tick() => {
                let payload = rng.gen::<u32>();
                last_ping = Some(payload);
                let data = payload.to_ne_bytes().into_iter().collect::<Vec<_>>();
                tracing::trace!(data = ?&data[..], "Sending ping");
                sender.send(Message::Ping(data)).await?;
                ping_interval.reset();
            }
            event = system_events.recv() => {
                let Ok(event) = event else {
                    break Some((CLOSE_NORMAL, "system shutting down"));
                };

                if let Err(error) = system_event(&mut sender, event).await {
                    tracing::error!(?error, "Failed to process system event");
                };
            }
            message = receiver.next() => {
                let Some(message) = message else {
                    break None;
                };

                match message? {
                    Message::Text(_) => break Some((CLOSE_PROTOCOL_ERROR, "unsupported message")),
                    Message::Binary(_) => break Some((CLOSE_PROTOCOL_ERROR, "unsupported message")),
                    Message::Ping(payload) => {
                        sender.send(Message::Pong(payload)).await?;
                        continue;
                    },
                    Message::Pong(data) => {
                        tracing::trace!(data = ?&data[..], "Pong");

                        let Some(expected) = last_ping else {
                            continue;
                        };

                        if expected.to_ne_bytes()[..] != data[..] {
                            continue;
                        }

                        close_interval.reset();
                        ping_interval.reset();
                        last_ping = None;
                    },
                    Message::Close(_) => break None,
                }
            }
        }
    };

    if let Some((code, reason)) = close_here {
        tracing::info!(code, reason, "Closing websocket with reason");

        sender
            .send(Message::Close(Some(CloseFrame {
                code,
                reason: Cow::Borrowed(reason),
            })))
            .await?;
    } else {
        tracing::info!("Closing websocket");
    };

    Ok(())
}