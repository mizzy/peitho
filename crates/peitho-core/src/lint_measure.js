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

  function waitForFonts() {
    if (!document.fonts || !document.fonts.ready) {
      return Promise.resolve();
    }
    // Bound the wait: document.fonts.ready has been observed to hang
    // indefinitely under Chrome's --virtual-time-budget on Linux headless
    // (same pitfall class as image decode promises). If fonts don't settle
    // in time, publish measurements against whatever font resolves at the
    // next requestAnimationFrame; better than emitting no payload.
    return Promise.race([
      document.fonts.ready.then(function () {}, function () {}),
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
      expandBounds(bounds, rect);
    });

    return bounds;
  }

  function clipsOverflow(value) {
    return value === "hidden" || value === "auto" || value === "scroll" ||
      value === "clip";
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
    var worstByAxis = {
      horizontal: null,
      vertical: null
    };

    walkDescendants(slide, function (element) {
      var style = getComputedStyle(element);
      if (isVisuallyHidden(style)) {
        return;
      }

      function consider(axis, overflowValue, overflowPx) {
        var worst = worstByAxis[axis];
        if (
          !clipsOverflow(overflowValue) ||
          overflowPx <= 0 ||
          (worst !== null && overflowPx <= worst.slotOverflowPx)
        ) {
          return;
        }
        worstByAxis[axis] = {
          slotOverflowAxis: axis,
          slotOverflowPx: overflowPx,
          slotOverflowValue: overflowValue,
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
        slotName: slotNameFor(worst.element, slide)
      });
    }
    if (worstByAxis.horizontal !== null) {
      emit(worstByAxis.horizontal);
    }
    if (worstByAxis.vertical !== null) {
      emit(worstByAxis.vertical);
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

  function measureTextFont(slide) {
    var walker = document.createTreeWalker(slide, NodeFilter.SHOW_TEXT);
    var minFontSizePx = null;
    var minFontSample = null;
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
      if (minFontSizePx === null || size < minFontSizePx) {
        minFontSizePx = size;
        minFontSample = sample;
      }
    }

    return {
      minFontSizePx: minFontSizePx,
      minFontSample: minFontSample
    };
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

  function publish(results) {
    var payload = base64EncodeUtf8(JSON.stringify(results));
    var total = Math.max(1, Math.ceil(payload.length / CHUNK_SIZE));
    for (var index = 0; index < total; index += 1) {
      console.log(
        CHUNK + " " + (index + 1) + "/" + total + " " +
          payload.slice(index * CHUNK_SIZE, (index + 1) * CHUNK_SIZE)
      );
    }
    console.log(DONE);
  }

  waitForWindowLoad()
    .then(waitForImages)
    .then(waitForFonts)
    .then(waitForFrame)
    .then(function () {
      publish(measureSlides());
    });
})();
