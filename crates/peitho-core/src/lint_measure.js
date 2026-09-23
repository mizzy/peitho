(function () {
  var CHUNK = "PEITHO_LINT_" + "CHUNK";
  var DONE = "PEITHO_LINT_" + "DONE";
  // Keep each console payload comfortably below macOS PIPE_BUF after Chrome's log wrapper.
  var CHUNK_SIZE = 300;
  // Do not wait unboundedly for window.load: it includes font fetches even with
  // font-display: swap, and virtual time can outrun real-time resource work under
  // Chrome's --virtual-time-budget, making Chrome exit before lint publishes.
  var WINDOW_LOAD_TIMEOUT_MS = 2000;
  var FONT_READY_TIMEOUT_MS = 2000; // Below Chrome --virtual-time-budget=10000.

  function waitForWindowLoad() {
    if (document.readyState === "complete") {
      return Promise.resolve();
    }
    return Promise.race([
      new Promise(function (resolve) {
        window.addEventListener("load", resolve, { once: true });
      }),
      new Promise(function (resolve) {
        setTimeout(resolve, WINDOW_LOAD_TIMEOUT_MS);
      })
    ]);
  }

  function waitForImage(image) {
    if (image.complete) {
      return Promise.resolve();
    }
    return new Promise(function (resolve) {
      image.addEventListener("load", resolve, { once: true });
      image.addEventListener("error", resolve, { once: true });
    });
  }

  function waitForImages() {
    return Promise.all(Array.prototype.map.call(document.images, waitForImage));
  }

  function settleFontPromise(promise) {
    return promise.then(function () {}, function () {});
  }

  function firstFontRangeValue(value) {
    return String(value || "normal").trim().split(/\s+/)[0];
  }

  function fontStyleValue(value) {
    var parts = String(value || "normal").trim().split(/\s+/);
    if (parts[0] === "oblique" && parts.length > 1) {
      return parts.slice(0, 2).join(" ");
    }
    return parts[0];
  }

  function fontShorthand(face) {
    var values = [];
    var style = fontStyleValue(face.style);
    var weight = firstFontRangeValue(face.weight);
    var stretch = firstFontRangeValue(face.stretch || face.width);

    if (style !== "normal") {
      values.push(style);
    }
    if (weight !== "normal") {
      values.push(weight);
    }
    if (stretch !== "normal") {
      values.push(stretch);
    }
    values.push("16px");
    values.push(face.family);
    return values.join(" ");
  }

  function requestDeclaredFonts() {
    if (!document.fonts || !document.fonts.load || !document.fonts.forEach) {
      return Promise.resolve();
    }

    var loads = [];
    document.fonts.forEach(function (face) {
      loads.push(settleFontPromise(document.fonts.load(fontShorthand(face))));
      // FontFaceSet.load defaults to a space sample, which unicode-subset faces
      // may exclude. Loading the enumerated face directly guarantees that the
      // declaration itself is requested; repeated loads share its status promise.
      if (face.load) {
        loads.push(settleFontPromise(face.load()));
      }
    });
    return Promise.all(loads).then(function () {
      if (document.fonts.ready) {
        return settleFontPromise(document.fonts.ready);
      }
    });
  }

  function waitForFonts(declaredFontsReady) {
    // Bound the wait: explicit font loads and document.fonts.ready can hang
    // indefinitely under Chrome's --virtual-time-budget on Linux headless
    // (same pitfall class as image decode promises). If fonts don't settle
    // in time, publish measurements against whatever font resolves at the
    // next requestAnimationFrame; better than emitting no payload.
    return Promise.race([
      declaredFontsReady,
      new Promise(function (resolve) {
        setTimeout(resolve, FONT_READY_TIMEOUT_MS);
      })
    ]);
  }

  function waitForFrame() {
    return new Promise(function (resolve) {
      requestAnimationFrame(function () {
        resolve();
      });
    });
  }

  function expandBounds(bounds, rect) {
    bounds.minLeft = Math.min(bounds.minLeft, rect.left);
    bounds.minTop = Math.min(bounds.minTop, rect.top);
    bounds.maxRight = Math.max(bounds.maxRight, rect.right);
    bounds.maxBottom = Math.max(bounds.maxBottom, rect.bottom);
  }

  function walkDescendants(element, visit) {
    Array.prototype.forEach.call(element.children, function (child) {
      visit(child);
      walkDescendants(child, visit);
    });
  }

  function contentBounds(slide, slideRect) {
    var bounds = {
      minLeft: slideRect.left,
      minTop: slideRect.top,
      maxRight: slideRect.right,
      maxBottom: slideRect.bottom
    };

    walkDescendants(slide, function (element) {
      var rect = element.getBoundingClientRect();
      if (rect.width === 0 && rect.height === 0) {
        return;
      }
      if (isClippedInside(element, slide)) {
        // The clipper's scrollWidth/scrollHeight report loss past its end/bottom
        // edge; loss past its start/top edge is invisible to them, so keep those.
        bounds.minLeft = Math.min(bounds.minLeft, rect.left);
        bounds.minTop = Math.min(bounds.minTop, rect.top);
        return;
      }
      expandBounds(bounds, rect);
    });

    return bounds;
  }

  function clipsOverflow(value) {
    return value === "hidden" || value === "auto" || value === "scroll" ||
      value === "clip";
  }

  function isClippedInside(element, slide) {
    var current = element;
    while (current && current !== slide) {
      var style = getComputedStyle(current);
      // Absolute/fixed elements follow offsetParent through their containing-block
      // chain; a null or out-of-slide offsetParent counts the rect conservatively.
      var next = (style.position === "absolute" || style.position === "fixed")
        ? current.offsetParent : current.parentElement;
      if (!next || !slide.contains(next)) {
        return false;
      }
      if (next !== slide) {
        var nextStyle = getComputedStyle(next);
        if (clipsOverflow(nextStyle.overflowX) || clipsOverflow(nextStyle.overflowY)) {
          return true;
        }
      }
      current = next;
    }
    return false;
  }

  function ownsEllipsizableInlineContent(element) {
    return Array.prototype.every.call(element.children, function (child) {
      var childDisplay = getComputedStyle(child).display;
      if (childDisplay === "none") {
        return true;
      }
      return childDisplay === "inline" &&
        !/^(img|svg|video|audio|canvas|iframe|object|embed|input|select|textarea|button|math)$/i.test(child.tagName) &&
        ownsEllipsizableInlineContent(child);
    });
  }

  function isTextTruncation(element, style, overflowValue) {
    if (overflowValue !== "hidden" && overflowValue !== "clip") {
      return false;
    }
    if (style.textOverflow === "clip") {
      return false;
    }
    if (
      style.display === "flex" ||
      style.display === "inline-flex" ||
      style.display === "grid" ||
      style.display === "inline-grid"
    ) {
      return false;
    }
    return ownsEllipsizableInlineContent(element);
  }

  function isVisuallyHidden(style) {
    return style.visibility === "hidden" || style.visibility === "collapse";
  }

  function slotNameOn(element) {
    for (var index = 0; index < element.classList.length; index += 1) {
      var className = element.classList.item(index);
      if (className.indexOf("slot-") === 0 && className.length > 5) {
        return className.slice(5);
      }
    }
    return null;
  }

  function slotNameFor(element, slide) {
    var current = element;
    while (current) {
      var name = slotNameOn(current);
      if (name !== null) {
        return name;
      }
      if (current === slide) {
        break;
      }
      current = current.parentElement;
    }

    // Shipped themes commonly put clipping on a wrapper around the slot element.
    // Name a descendant slot only when the clip unambiguously belongs to one slot.
    var descendantNames = [];
    walkDescendants(element, function (descendant) {
      var descendantName = slotNameOn(descendant);
      if (descendantName !== null) {
        descendantNames.push(descendantName);
      }
    });
    return descendantNames.length === 1 ? descendantNames[0] : null;
  }

  function measureSlotOverflows(slide) {
    var worstByKind = {
      horizontal: null,
      vertical: null,
      truncation: null
    };

    walkDescendants(slide, function (element) {
      var style = getComputedStyle(element);
      if (isVisuallyHidden(style)) {
        return;
      }

      function consider(axis, overflowValue, overflowPx) {
        var truncated = axis === "horizontal" &&
          isTextTruncation(element, style, overflowValue);
        var kind = truncated ? "truncation" : axis;
        var worst = worstByKind[kind];
        if (
          !clipsOverflow(overflowValue) ||
          overflowPx <= 0 ||
          (worst !== null && overflowPx <= worst.slotOverflowPx)
        ) {
          return;
        }
        worstByKind[kind] = {
          slotOverflowAxis: axis,
          slotOverflowPx: overflowPx,
          slotOverflowValue: overflowValue,
          slotOverflowTruncated: truncated,
          element: element
        };
      }

      // Accessibility copies such as KaTeX's MathML are collapsed to a 1x1px
      // box. Skip only the collapsed axis so overflow on the other remains visible.
      if (element.clientWidth > 1) {
        consider(
          "horizontal",
          style.overflowX,
          element.scrollWidth - element.clientWidth
        );
      }
      if (element.clientHeight > 1) {
        consider(
          "vertical",
          style.overflowY,
          element.scrollHeight - element.clientHeight
        );
      }
    });

    var overflows = [];
    function emit(worst) {
      overflows.push({
        slotOverflowAxis: worst.slotOverflowAxis,
        slotOverflowPx: worst.slotOverflowPx,
        slotOverflowValue: worst.slotOverflowValue,
        slotOverflowTruncated: worst.slotOverflowTruncated,
        slotName: slotNameFor(worst.element, slide)
      });
    }
    if (worstByKind.horizontal !== null) {
      emit(worstByKind.horizontal);
    }
    if (worstByKind.vertical !== null) {
      emit(worstByKind.vertical);
    }
    if (worstByKind.truncation !== null) {
      emit(worstByKind.truncation);
    }
    return overflows;
  }

  function truncateSample(sample) {
    if (Array.from(sample).length > 40) {
      return Array.from(sample).slice(0, 40).join("") + "…";
    }
    return sample;
  }

  function fontSample(text) {
    var sample = text.replace(/\s+/g, " ").trim();
    return truncateSample(sample);
  }

  var LINT_MIN_FONT_SIZE_PROPERTY = "--peitho-lint-min-font-size";

  // Returns the floor in px, null when unset, or NaN when the value is not a
  // pt/px length (the caller reports the raw value as an error).
  function parseLintMinFontSizePx(value) {
    var raw = value.trim();
    if (raw === "") {
      return null;
    }
    if (raw === "0") {
      return 0;
    }
    var match = /^(\d+(?:\.\d+)?)(pt|px)$/i.exec(raw);
    if (match === null) {
      return NaN;
    }
    var number = parseFloat(match[1]);
    return match[2].toLowerCase() === "pt" ? number / 0.75 : number;
  }

  function measureTextFont(slide) {
    var walker = document.createTreeWalker(slide, NodeFilter.SHOW_TEXT);
    var minFontSizePx = null;
    var minFontSample = null;
    var waiver = null;
    var waiverError = null;
    var node;

    while ((node = walker.nextNode())) {
      var sample = fontSample(node.textContent || "");
      var parent = node.parentElement;
      if (sample === "" || !parent) {
        continue;
      }
      if (parent.closest(".peitho-footnotes, sup.peitho-footnote-ref")) {
        continue;
      }

      var rect = parent.getBoundingClientRect();
      var style = getComputedStyle(parent);
      var visibility = style.visibility;
      if (
        (rect.width === 0 && rect.height === 0) ||
        visibility === "hidden" ||
        visibility === "collapse"
      ) {
        continue;
      }

      var size = parseFloat(style.fontSize);
      if (!isFinite(size)) {
        continue;
      }
      var floorValue = style.getPropertyValue(LINT_MIN_FONT_SIZE_PROPERTY);
      var floorPx = parseLintMinFontSizePx(floorValue);
      if (floorPx === null) {
        if (minFontSizePx === null || size < minFontSizePx) {
          minFontSizePx = size;
          minFontSample = sample;
        }
        continue;
      }
      if (isNaN(floorPx)) {
        if (waiverError === null) {
          waiverError = floorValue.trim();
        }
        continue;
      }
      // A node below its own floor always outranks an allowed one, and among
      // violations the deepest one wins, so a violation is never hidden.
      var candidate = {
        fontSizePx: size,
        sample: sample,
        thresholdPx: floorPx
      };
      if (waiver === null || compareWaivers(candidate, waiver) < 0) {
        waiver = candidate;
      }
    }

    return {
      minFontSizePx: minFontSizePx,
      minFontSample: minFontSample,
      fontSizeWaiver: waiver,
      fontSizeWaiverError: waiverError
    };
  }

  function compareWaivers(a, b) {
    var deltaA = a.fontSizePx - a.thresholdPx;
    var deltaB = b.fontSizePx - b.thresholdPx;
    var violatesA = deltaA < 0;
    var violatesB = deltaB < 0;
    if (violatesA !== violatesB) {
      return violatesA ? -1 : 1;
    }
    if (violatesA) {
      return deltaA - deltaB;
    }
    return a.fontSizePx - b.fontSizePx;
  }

  function measureSlide(slide, index) {
    var slideRect = slide.getBoundingClientRect();
    var bounds = contentBounds(slide, slideRect);
    var textFont = measureTextFont(slide);
    var slotOverflows = measureSlotOverflows(slide);

    return {
      slide: index + 1,
      contentWidth: Math.max(bounds.maxRight - bounds.minLeft, slide.scrollWidth),
      contentHeight: Math.max(bounds.maxBottom - bounds.minTop, slide.scrollHeight),
      boxWidth: slideRect.width,
      boxHeight: slideRect.height,
      minFontSizePx: textFont.minFontSizePx,
      minFontSample: textFont.minFontSample,
      fontSizeWaiver: textFont.fontSizeWaiver,
      fontSizeWaiverError: textFont.fontSizeWaiverError,
      slotOverflows: slotOverflows
    };
  }

  function measureSlides() {
    return Array.prototype.map.call(
      document.querySelectorAll("section.peitho-slide"),
      measureSlide
    );
  }

  function base64EncodeUtf8(text) {
    var bytes = new TextEncoder().encode(text);
    var binary = "";
    for (var index = 0; index < bytes.length; index += 1) {
      binary += String.fromCharCode(bytes[index]);
    }
    return btoa(binary);
  }

  var published = false;

  function publish(results) {
    if (published) {
      return;
    }
    published = true;
    var payload = base64EncodeUtf8(JSON.stringify({
      shadowMountedError: document.documentElement.getAttribute(
        "data-peitho-shadow-mounted-error"
      ),
      slides: results
    }));
    var total = Math.max(1, Math.ceil(payload.length / CHUNK_SIZE));
    for (var index = 0; index < total; index += 1) {
      console.log(
        CHUNK + " " + (index + 1) + "/" + total + " " +
          payload.slice(index * CHUNK_SIZE, (index + 1) * CHUNK_SIZE)
      );
    }
    console.log(DONE);
  }

  // Start every declared face while this parser-blocking inline script is running,
  // before Chrome can consider the lint page complete.
  var declaredFontsReady = requestDeclaredFonts();

  // Chrome prints when it considers the page ready and exits immediately after,
  // so the readiness chain below is racing that teardown: bounding its waits
  // shortens the race but never orders it, and losing means no payload at all.
  // beforeprint runs after layout settles and before the PDF bytes are written,
  // so it publishes even when a wait never settles. The latch in publish() keeps
  // whichever path arrives first as the single payload.
  window.addEventListener("beforeprint", function () {
    publish(measureSlides());
  });

  waitForWindowLoad()
    .then(waitForImages)
    .then(function () {
      return waitForFonts(declaredFontsReady);
    })
    .then(waitForFrame)
    .then(function () {
      publish(measureSlides());
    });
})();
