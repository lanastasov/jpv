//! Module which performs verb inflection, based on a words class.

use std::collections::BTreeMap;

use fixed_map::Set;
use musli::{Decode, Encode};
use musli_zerocopy::ZeroCopy;
use serde::{Deserialize, Serialize};

use crate::elements::Entry;
use crate::inflection::{godan, Inflections};
use crate::kana::{Fragments, Full};
use crate::PartOfSpeech;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Encode,
    Decode,
    ZeroCopy,
)]
#[repr(u8)]
#[zero_copy(bounds = {T: ZeroCopy})]
#[musli(bound = {T: Encode<M>}, decode_bound = {T: Decode<'de, M>})]
pub enum ReadingOption<T> {
    None,
    Some(T),
}

/// The reading which this set of inflections belong to.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    Encode,
    Decode,
    ZeroCopy,
)]
#[repr(C)]
pub struct Reading {
    /// Index of the kanji that the reading matches, if one is present.
    pub kanji: u8,
    /// Index of the reading used.
    pub reading: u8,
}

/// The kind of word.
pub enum Kind {
    Verb,
    Adjective,
}

/// Try to conjugate the given entry as a verb.
pub fn conjugate<'a>(entry: &Entry<'a>) -> Vec<(Reading, Inflections<'a>, Kind)> {
    let mut output = Vec::new();

    let readings = reading_permutations(entry);

    for pos in parts_of_speech(entry) {
        for &(kanji, reading) in &readings {
            let (_, kanji_text) = kanji.unwrap_or(reading);
            let (_, reading_text) = reading;

            let mut inflections;
            let kind;
            let de_conjugation;
            let stem;

            match pos {
                PartOfSpeech::VerbIchidan | PartOfSpeech::VerbIchidanS => {
                    let (Some(k), Some(r)) = (
                        kanji_text.strip_suffix('る'),
                        reading_text.strip_suffix('る'),
                    ) else {
                        continue;
                    };

                    inflections = inflections! {
                        k, r,
                        [Te], ("て"),
                    };

                    macro_rules! populate {
                        ($suffix:expr $(, $inflect:ident)*) => {
                            inflections.insert(inflect!($($inflect),*), Fragments::new([k], [r], [$suffix]));
                        }
                    }

                    ichidan!(populate);
                    kind = Kind::Verb;
                    de_conjugation = false;
                    stem = Fragments::new([k], [r], ["っ"]);
                }
                PartOfSpeech::VerbGodanKS => {
                    let (Some(k), Some(r)) = (
                        kanji_text.strip_suffix('く'),
                        reading_text.strip_suffix('く'),
                    ) else {
                        continue;
                    };

                    let g = godan::IKU;

                    inflections = BTreeMap::new();
                    inflections.insert(inflect!(Te), Fragments::new([k], [r], [g.te]));

                    macro_rules! populate {
                        ($suffix:expr $(, $inflect:ident)*) => {
                            inflections.insert(inflect!($($inflect),*), Fragments::new([k], [r], $suffix));
                        }
                    }

                    godan!(populate, g);

                    kind = Kind::Verb;
                    de_conjugation = g.de;
                    stem = Fragments::new([k], [r], [g.te_stem]);
                }
                PartOfSpeech::VerbGodanAru
                | PartOfSpeech::VerbGodanB
                | PartOfSpeech::VerbGodanG
                | PartOfSpeech::VerbGodanK
                | PartOfSpeech::VerbGodanM
                | PartOfSpeech::VerbGodanN
                | PartOfSpeech::VerbGodanR
                | PartOfSpeech::VerbGodanRI
                | PartOfSpeech::VerbGodanS
                | PartOfSpeech::VerbGodanT
                | PartOfSpeech::VerbGodanU
                | PartOfSpeech::VerbGodanUS
                | PartOfSpeech::VerbGodanUru => {
                    let mut k = kanji_text.chars();
                    let mut r = reading_text.chars();

                    let g = match k.next_back() {
                        Some('う') => godan::U,
                        Some('つ') => godan::TSU,
                        Some('る') => godan::RU,
                        Some('く') => godan::KU,
                        Some('ぐ') => godan::GU,
                        Some('む') => godan::MU,
                        Some('ぶ') => godan::BU,
                        Some('ぬ') => godan::NU,
                        Some('す') => godan::SU,
                        _ => continue,
                    };

                    r.next_back();

                    let k = k.as_str();
                    let r = r.as_str();

                    inflections = BTreeMap::new();
                    inflections.insert(inflect!(Te), Fragments::new([k], [r], [g.te]));

                    macro_rules! populate {
                        ($suffix:expr $(, $inflect:ident)*) => {
                            inflections.insert(inflect!($($inflect),*), Fragments::new([k], [r], $suffix));
                        }
                    }

                    godan!(populate, g);

                    kind = Kind::Verb;
                    de_conjugation = g.de;
                    stem = Fragments::new([k], [r], [g.te_stem]);
                }
                PartOfSpeech::VerbSuruSpecial | PartOfSpeech::VerbSuruIncluded => {
                    let mut kanji = kanji_text.char_indices();
                    let mut reading = reading_text.char_indices();

                    let (Some((k_e, 'る')), Some((_, 'る'))) =
                        (kanji.next_back(), reading.next_back())
                    else {
                        continue;
                    };

                    let (Some((_, k)), Some((_, 'す'))) = (kanji.next_back(), reading.next_back())
                    else {
                        continue;
                    };

                    let kanji_prefix = kanji.as_str();
                    let reading_prefix = reading.as_str();
                    let kanji_stem = &kanji_text[..k_e];

                    inflections = BTreeMap::new();

                    if k == 'す' {
                        macro_rules! populate {
                            ($prefix:expr, $suffix:expr $(, $inflect:ident)*) => {
                                inflections.insert(inflect!($($inflect),*), Fragments::new([kanji_prefix], [reading_prefix], [concat!($prefix, $suffix)]));
                            }
                        }

                        suru!(populate);
                    } else {
                        macro_rules! populate {
                            ($prefix:expr, $suffix:expr $(, $inflect:ident)*) => {
                                inflections.insert(inflect!($($inflect),*), Fragments::new([kanji_stem], [reading_prefix, $prefix], [$suffix]));
                            }
                        }

                        suru!(populate);
                    }

                    kind = Kind::Verb;
                    de_conjugation = false;
                    stem = Fragments::default();
                }
                PartOfSpeech::VerbKuru => {
                    let mut kanji = kanji_text.char_indices();
                    let mut reading = reading_text.char_indices();

                    let (Some((k_e, 'る')), Some((_, 'る'))) =
                        (kanji.next_back(), reading.next_back())
                    else {
                        continue;
                    };

                    let (Some((_, k)), Some((_, 'く'))) = (kanji.next_back(), reading.next_back())
                    else {
                        continue;
                    };

                    let kanji_prefix = kanji.as_str();
                    let reading_prefix = reading.as_str();
                    let kanji_stem = &kanji_text[..k_e];

                    inflections = BTreeMap::new();

                    if k == 'く' {
                        macro_rules! populate {
                            ($prefix:expr, $suffix:expr $(, $inflect:ident)*) => {
                                inflections.insert(inflect!($($inflect),*), Fragments::new([kanji_prefix], [reading_prefix], [concat!($prefix, $suffix)]));
                            }
                        }

                        kuru!(populate);
                    } else {
                        macro_rules! populate {
                            ($prefix:expr, $suffix:expr $(, $inflect:ident)*) => {
                                inflections.insert(inflect!($($inflect),*), Fragments::new([kanji_stem], [reading_prefix, $prefix], [$suffix]));
                            }
                        }

                        kuru!(populate);
                    }

                    kind = Kind::Verb;
                    de_conjugation = false;
                    stem = Fragments::default();
                }
                PartOfSpeech::AdjectiveI => {
                    let (Some(k), Some(r)) = (
                        kanji_text.strip_suffix('い'),
                        reading_text.strip_suffix('い'),
                    ) else {
                        continue;
                    };

                    inflections = inflections! {
                        k, r,
                        [], ("い"),
                        [Polite], ("いです"),
                        [Past], ("かった"),
                        [Past, Polite], ("かったです"),
                        [Negative], ("くない"),
                        [Negative, Polite], ("くないです"),
                        [Past, Negative], ("なかった"),
                        [Past, Negative, Polite], ("なかったです"),
                    };

                    kind = Kind::Adjective;
                    de_conjugation = false;
                    stem = Fragments::default();
                }
                PartOfSpeech::AdjectiveIx => {
                    let (Some(k), Some(r)) = (
                        kanji_text.strip_suffix("いい"),
                        reading_text.strip_suffix("いい"),
                    ) else {
                        continue;
                    };

                    inflections = inflections! {
                        k, r,
                        [], ("いい"),
                        [Polite], ("いいです"),
                        [Past], ("よかった"),
                        [Past, Polite], ("よかったです"),
                        [Negative], ("よくない"),
                        [Negative, Polite], ("よくないです"),
                        [Past, Negative], ("よなかった"),
                        [Past, Negative, Polite], ("よなかったです"),
                    };

                    kind = Kind::Adjective;
                    de_conjugation = false;
                    stem = Fragments::default();
                }
                PartOfSpeech::AdjectiveNa => {
                    inflections = inflections! {
                        kanji_text, reading_text,
                        [], ("だ"),
                        [Polite], ("です"),
                        [Past], ("だった"),
                        [Past, Polite], ("でした"),
                        [Negative], ("ではない"),
                        [Negative, Polite], ("ではありません"),
                        [Past, Negative], ("ではなかった"),
                        [Past, Negative, Polite], ("ではありませんでした"),
                    };

                    kind = Kind::Adjective;
                    de_conjugation = false;
                    stem = Fragments::default();
                }
                _ => {
                    continue;
                }
            };

            if let Some(p) = inflections.get(&inflect!(Te)).cloned() {
                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeIru, Te $(, $inflect)*), p.concat([concat!("い", $suffix)]));
                    }
                }

                inflections.insert(inflect!(TeIru, Te, Short), p.concat(["る"]));
                ichidan!(populate);

                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeAru, Te $(, $inflect)*), p.concat($suffix));
                    }
                }

                godan!(populate, godan::RU, "あ");

                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeIku, Te $(, $inflect)*), p.concat($suffix));
                    }
                }

                godan!(populate, godan::IKU, "い");

                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeShimau, Te $(, $inflect)*), p.concat($suffix));
                    }
                }

                godan!(populate, godan::U, "しま");

                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeOku, Te $(, $inflect)*), p.concat($suffix));
                    }
                }

                godan!(populate, godan::KU, "お");
                inflections.insert(inflect!(Te, TeOku, Short), p.concat(["く"]));

                macro_rules! populate {
                    ($r:expr, $suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(TeKuru, Te $(, $inflect)*), p.concat([concat!($r, $suffix)]));
                    }
                }

                kuru!(populate);
            }

            if !stem.is_empty() {
                macro_rules! populate {
                    ($suffix:expr $(, $inflect:ident)*) => {
                        inflections.insert(inflect!(Chau $(, $inflect)*), stem.concat($suffix));
                    }
                }

                if de_conjugation {
                    godan!(populate, godan::U, "じゃ");
                } else {
                    godan!(populate, godan::U, "ちゃ");
                }
            }

            let reading = Reading {
                kanji: kanji.map(|(i, _)| i as u8).unwrap_or(u8::MAX),
                reading: reading.0 as u8,
            };

            let inflections = Inflections {
                dictionary: Full::new(kanji_text, reading_text, ""),
                inflections,
            };

            output.push((reading, inflections, kind));
        }
    }

    output
}

pub(crate) fn reading_permutations<'a>(
    entry: &Entry<'a>,
) -> Vec<(Option<(usize, &'a str)>, (usize, &'a str))> {
    let mut readings = Vec::new();

    for (reading_index, reading) in entry.reading_elements.iter().enumerate() {
        if reading.no_kanji || entry.kanji_elements.is_empty() {
            readings.push((None, (reading_index, reading.text)));
            continue;
        }

        for (kanji_index, kanji) in entry.kanji_elements.iter().enumerate() {
            if reading.applies_to(&kanji.text) {
                readings.push((
                    Some((kanji_index, kanji.text)),
                    (reading_index, reading.text),
                ));
            }
        }
    }

    readings
}

/// If the entry is a verb, figure out the verb kind.
pub(crate) fn parts_of_speech(entry: &Entry<'_>) -> Set<PartOfSpeech> {
    let mut pos = Set::new();

    for sense in &entry.senses {
        for p in sense.pos {
            pos.insert(p);
        }
    }

    pos
}