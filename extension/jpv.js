const DEBUG = false;
const WIDTH = 400;
const HEIGHT = 600;
const PADDING = 10;
const SELECT = true;
const FOLLOWMOUSE = false;
const MAX_X_OFFSET = 1024;

let iframe = null;
let loadListener = null;
let currentText = null;

function isValidStart(el) {
    return el.localName !== "body";
}

function isInlineElement(el) {
    let style = window.getComputedStyle(el);
    return style.display === "inline" || style.display === "inline-block";
}

/**
 * @returns {Element | null} The bounding element or null if it contains no text.
 */
function getBoundingElement(el) {
    if (!el.textContent) {
        return null;
    }

    let current = el;

    if (!isValidStart(current)) {
        return null;
    }

    if (isInlineElement(current)) {
        while (isInlineElement(current.parentNode)) {
            current = current.parentNode;
        }

        if (current.parentNode) {
            current = current.parentNode;
        }
    }

    return current;
}

function closeWindow() {
    if (!loadListener) {
        return false;
    }

    iframe.removeEventListener('load', loadListener);
    loadListener = null;
    iframe.classList.remove('active');
    iframe.src = '';
    currentText = null;
    return true;
}

/**
 * Narrows the specified range until it fits a natural word boundary.
 *
 * This is a fairly tricky operation to perform over a DOM, because it contains
 * a bunch of mixed elements, and ranges operate over them.
 *
 * We start by narrowing the range from the right, we have to scan for the last
 * referenced text node, check if it contains a natural paragraph boundary (the
 * various forms of dots, exclamations, ...).
 *
 * Then we repeat the operation from the left.
 *
 * @param {Range} range The range to narrow, until it fits a natural text
 * boundary which is pointed to by the cursor.
 */
function adjustRangeToBoundaries(range, x, y) {
    let update = false;
    let boundaries = walk(range);

    if (boundaries.length > 0) {
        let current = range.cloneRange();

        let start = 0;
        let end = boundaries.length - 1;

        while (start <= end) {
            let { node, index } = boundaries[start];
            current.setStart(node, index);

            if (!rectContainsAny(current.getClientRects(), x, y)) {
                break;
            }

            range.setStart(node, index);
            start += 1;
        }

        current.setStart(range.startContainer, range.startOffset);

        while (start <= end) {
            let { node, index } = boundaries[end];
            current.setEnd(node, index);

            if (!rectContainsAny(current.getClientRects(), x, y)) {
                break;
            }

            range.setEnd(node, index);
            end -= 1;
        }
    }

    return update;
}

/**
 * @param {Range} original Original range to scan.
 * @param {Factory}
 * @returns {Range} The walked range range, or null if no valid range was found.
 */
function walk(range) {
    let node = range.startContainer;
    let boundaries = new Boundaries();

    outer:
    while (node) {
        if (node.nodeType === Node.TEXT_NODE) {
            boundaries.populate(node);
        } else {
            if (node.firstChild !== null) {
                node = node.firstChild;
                continue;
            }

            if (node.nextSibling !== null) {
                node = node.nextSibling;
                continue;
            }
        }

        if (node === range.endContainer || node.parentNode === range.endContainer) {
            break;
        }

        node = node.parentNode.nextSibling;
    }

    return boundaries.output();
}

function rectContainsAny(rects, x, y) {
    for (let rect of rects) {
        if (rectContains(rect, x, y)) {
            return true;
        }
    }

    return false;
}

function rectContains(rect, x, y) {
    return rect.x <= x && rect.x + rect.width >= x && rect.y <= y && rect.y + rect.height >= y;
}

function windowPosition(rect, e) {
    let popupHeight = HEIGHT;
    let popupWidth = WIDTH;
    let padding = PADDING;

    let windowWidth = window.innerWidth;
    let windowHeight = window.innerHeight;

    if (!FOLLOWMOUSE) {
        let maxX = e.clientX + MAX_X_OFFSET;

        let pos = {
            x: Math.min(rect.x + rect.width + padding, maxX),
            y: rect.y,
        };

        let neededHeight = pos.y + popupHeight + padding;
        let neededWidth = pos.x + popupWidth + padding;

        if (neededHeight > windowHeight) {
            pos.y -= neededHeight - windowHeight;
        }

        if (neededWidth > windowWidth) {
            pos.x -= neededWidth - windowWidth;
        }

        return pos;
    }

    let pos = { x: e.clientX, y: e.clientY };

    let neededWidth = pos.x + popupWidth + padding * 2;
    let neededHeight = pos.y + popupHeight + padding * 2;

    if (DEBUG) {
        console.debug({ windowWidth, windowHeight });
        console.debug({ neededWidth, neededHeight });
        console.debug(pos);
    }

    if (neededWidth > windowWidth) {
        pos.x -= popupWidth + padding;
    } else {
        pos.x += padding;
    }

    if (neededHeight > windowHeight) {
        pos.y -= (neededHeight - windowHeight) - padding;
    } else {
        pos.y += padding;
    }

    if (pos.y < 0) {
        pos.y = padding;
    }

    return pos;
}

function openWindow(e) {
    let element = getBoundingElement(e.target);

    if (element == null) {
        return;
    }

    let textRange = document.createRange();
    textRange.selectNodeContents(element);

    adjustRangeToBoundaries(textRange, e.clientX, e.clientY);

    let pos = windowPosition(element.getBoundingClientRect(), e);
    let text = textRange.toString();

    if (SELECT) {
        let s = window.getSelection();

        if (s.rangeCount > 0) {
            let existing = s.getRangeAt(0);
            existing.setStart(textRange.startContainer, textRange.startOffset);
            existing.setEnd(textRange.endContainer, textRange.endOffset);

            for (let i = 1; i < s.rangeCount; i++) {
                s.removeRange(s.getRangeAt(i));
            }
        } else {
            s.addRange(textRange);
        }
    }

    if (DEBUG) {
        console.debug(pos);
    }

    if (currentText != text) {
        if (!loadListener) {
            loadListener = () => iframe.classList.add('active');
            iframe.addEventListener('load', loadListener);
        }

        iframe.src = 'http://localhost:44714?embed=yes&q=' + encodeURIComponent(text);
        currentText = text;
    }

    iframe.style.left = `${pos.x}px`;
    iframe.style.top = `${pos.y}px`;
    iframe.style.width = `${WIDTH}px`;
    iframe.style.height = `${HEIGHT}px`;
    return;
}

function click(e) {
    if (!e.shiftKey) {
        if (closeWindow()) {
            e.preventDefault();
        }

        return;
    }

    openWindow(e);
    e.preventDefault();
}

function mouseMove(e) {
    if (e.shiftKey) {
        openWindow(e);
        e.preventDefault();
    }
}

if (document.body) {
    let fragment = document.createDocumentFragment();
    iframe = fragment.appendChild(document.createElement('iframe'));

    // set the position to the
    iframe.classList.add('jpv-definitions');
    iframe.src = 'http://localhost:37719';

    document.body.appendChild(iframe);

    document.documentElement.addEventListener('click', click);
    document.documentElement.addEventListener('mousemove', mouseMove);
}