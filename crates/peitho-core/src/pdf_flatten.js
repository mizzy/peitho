(function () {
  var PSEUDOS = ["::before", "::after"];
  var SCALE = 2;
  var MAX_CANVAS_DIMENSION = 16384;
  var MAX_CANVAS_AREA = 268000000;
  var nextClassId = 0;
  var pseudoStyleElement = null;
  var rasterCache = new Map();

  function hasGradient(backgroundImage) {
    return String(backgroundImage || "").toLowerCase().indexOf("gradient(") !== -1;
  }

  function hasUrlLayer(backgroundImage) {
    return String(backgroundImage || "").toLowerCase().indexOf("url(") !== -1;
  }

  function hasFixedAttachment(computed) {
    return String(computed.backgroundAttachment || "").toLowerCase().indexOf("fixed") !== -1;
  }

  function hasTextBackgroundClip(computed) {
    return String(computed.backgroundClip || "").toLowerCase().indexOf("text") !== -1;
  }

  function hasNonNormalBlendMode(computed) {
    return String(computed.backgroundBlendMode || "normal")
      .toLowerCase()
      .split(",")
      .some(function (mode) {
        return mode.trim() !== "normal";
      });
  }

  function parsePixel(value) {
    var match = String(value || "").match(/^([0-9]+(?:\.[0-9]+)?)px$/);
    if (!match) return null;
    var parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function isZeroPixel(value) {
    var parsed = parsePixel(value);
    return parsed !== null && parsed === 0;
  }

  function boxInsetsAreZero(computed) {
    return [
      computed.paddingTop,
      computed.paddingRight,
      computed.paddingBottom,
      computed.paddingLeft,
      computed.borderTopWidth,
      computed.borderRightWidth,
      computed.borderBottomWidth,
      computed.borderLeftWidth,
    ].every(isZeroPixel);
  }

  function effectiveBackgroundClip(computed) {
    var clips = String(computed.backgroundClip || "border-box")
      .toLowerCase()
      .split(",")
      .map(function (clip) {
        return clip.trim();
      })
      .filter(Boolean);
    return clips.length === 0 ? "border-box" : clips[clips.length - 1];
  }

  function isFullyTransparentColor(value) {
    var color = String(value || "").trim().toLowerCase();
    if (color === "transparent") return true;
    if (/^rgba\([^)]*,\s*0(?:\.0+)?\s*\)$/.test(color)) return true;
    if (/^rgb\([^)]*\/\s*0(?:\.0+)?%?\s*\)$/.test(color)) return true;
    return false;
  }

  function backgroundColorClipWouldChange(computed) {
    return (
      !isFullyTransparentColor(computed.backgroundColor) &&
      effectiveBackgroundClip(computed) !== "border-box" &&
      !boxInsetsAreZero(computed)
    );
  }

  function pixelSum(computed, properties) {
    var total = 0;
    for (var i = 0; i < properties.length; i++) {
      var parsed = parsePixel(computed[properties[i]]);
      if (parsed === null) return null;
      total += parsed;
    }
    return total;
  }

  function elementBorderBoxSize(computed) {
    var width = parsePixel(computed.width);
    var height = parsePixel(computed.height);
    if (width === null || height === null) return null;

    var boxSizing = String(computed.boxSizing || "").toLowerCase();
    if (boxSizing === "content-box") {
      var horizontalInsets = pixelSum(computed, [
        "paddingLeft",
        "paddingRight",
        "borderLeftWidth",
        "borderRightWidth",
      ]);
      var verticalInsets = pixelSum(computed, [
        "paddingTop",
        "paddingBottom",
        "borderTopWidth",
        "borderBottomWidth",
      ]);
      if (horizontalInsets === null || verticalInsets === null) return null;
      width += horizontalInsets;
      height += verticalInsets;
    } else if (boxSizing !== "border-box") {
      return null;
    }

    return { width: width, height: height };
  }

  function snapshotComputed(computed) {
    return {
      backgroundImage: computed.backgroundImage,
      backgroundPosition: computed.backgroundPosition,
      backgroundSize: computed.backgroundSize,
      backgroundRepeat: computed.backgroundRepeat,
      backgroundOrigin: computed.backgroundOrigin,
      backgroundClip: computed.backgroundClip,
      paddingTop: computed.paddingTop,
      paddingRight: computed.paddingRight,
      paddingBottom: computed.paddingBottom,
      paddingLeft: computed.paddingLeft,
      borderTopWidth: computed.borderTopWidth,
      borderRightWidth: computed.borderRightWidth,
      borderBottomWidth: computed.borderBottomWidth,
      borderLeftWidth: computed.borderLeftWidth,
      borderTopStyle: computed.borderTopStyle,
      borderRightStyle: computed.borderRightStyle,
      borderBottomStyle: computed.borderBottomStyle,
      borderLeftStyle: computed.borderLeftStyle,
    };
  }

  function xmlAttribute(value) {
    return String(value || "")
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");
  }

  function renderStyle(computed, width, height) {
    var declarations = [
      ["box-sizing", "border-box"],
      ["width", width + "px"],
      ["height", height + "px"],
      ["padding-top", computed.paddingTop],
      ["padding-right", computed.paddingRight],
      ["padding-bottom", computed.paddingBottom],
      ["padding-left", computed.paddingLeft],
      ["border-top-width", computed.borderTopWidth],
      ["border-right-width", computed.borderRightWidth],
      ["border-bottom-width", computed.borderBottomWidth],
      ["border-left-width", computed.borderLeftWidth],
      ["border-top-style", computed.borderTopStyle],
      ["border-right-style", computed.borderRightStyle],
      ["border-bottom-style", computed.borderBottomStyle],
      ["border-left-style", computed.borderLeftStyle],
      ["border-color", "transparent"],
      ["background-image", computed.backgroundImage],
      ["background-position", computed.backgroundPosition],
      ["background-size", computed.backgroundSize],
      ["background-repeat", computed.backgroundRepeat],
      ["background-origin", computed.backgroundOrigin],
      ["background-clip", computed.backgroundClip],
    ];
    return declarations
      .map(function (declaration) {
        return declaration[0] + ": " + xmlAttribute(declaration[1]);
      })
      .join("; ");
  }

  function collectTargets() {
    var targets = [];
    var elements = document.querySelectorAll("*");
    elements.forEach(function (element) {
      try {
        var computed = getComputedStyle(element);
        if (
          hasGradient(computed.backgroundImage) &&
          !hasUrlLayer(computed.backgroundImage) &&
          !hasFixedAttachment(computed) &&
          !hasTextBackgroundClip(computed) &&
          !hasNonNormalBlendMode(computed) &&
          !backgroundColorClipWouldChange(computed)
        ) {
          var rects = element.getClientRects();
          var size = rects.length === 1 ? elementBorderBoxSize(computed) : null;
          if (size !== null && size.width > 0 && size.height > 0) {
            targets.push({
              kind: "element",
              element: element,
              computed: snapshotComputed(computed),
              width: size.width,
              height: size.height,
            });
          }
        }
      } catch (_) {
        return;
      }

      PSEUDOS.forEach(function (pseudo) {
        try {
          var computed = getComputedStyle(element, pseudo);
          if (!hasGradient(computed.backgroundImage)) return;
          if (hasUrlLayer(computed.backgroundImage) || hasFixedAttachment(computed)) return;
          if (hasTextBackgroundClip(computed) || hasNonNormalBlendMode(computed)) return;
          var width = parsePixel(computed.width);
          var height = parsePixel(computed.height);
          if (width === null || height === null || width <= 0 || height <= 0) return;
          if (!boxInsetsAreZero(computed)) return;
          targets.push({
            kind: "pseudo",
            element: element,
            pseudo: pseudo,
            computed: snapshotComputed(computed),
            width: width,
            height: height,
          });
        } catch (_) {
          return;
        }
      });
    });
    return targets;
  }

  function loadImage(src) {
    return new Promise(function (resolve, reject) {
      var image = new Image();
      image.onload = function () {
        resolve(image);
      };
      image.onerror = reject;
      image.src = src;
    });
  }

  async function rasterizeBackground(computed, width, height) {
    var style = renderStyle(computed, width, height);
    var cacheKey = style;
    if (rasterCache.has(cacheKey)) return rasterCache.get(cacheKey);

    var canvasWidth = width * SCALE;
    var canvasHeight = height * SCALE;
    if (
      canvasWidth > MAX_CANVAS_DIMENSION ||
      canvasHeight > MAX_CANVAS_DIMENSION ||
      canvasWidth * canvasHeight > MAX_CANVAS_AREA
    ) {
      throw new Error("canvas too large for pdf gradient flatten: " + canvasWidth + "x" + canvasHeight);
    }

    var html =
      '<div xmlns="http://www.w3.org/1999/xhtml" style="' +
      style +
      '"></div>';
    var svg =
      '<svg xmlns="http://www.w3.org/2000/svg" width="' +
      width +
      '" height="' +
      height +
      '"><foreignObject width="100%" height="100%">' +
      html +
      "</foreignObject></svg>";
    var image = await loadImage("data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg));
    var canvas = document.createElement("canvas");
    canvas.width = canvasWidth;
    canvas.height = canvasHeight;
    var context = canvas.getContext("2d");
    if (!context) throw new Error("2d canvas context unavailable");
    context.scale(SCALE, SCALE);
    context.drawImage(image, 0, 0, width, height);
    var dataUrl = canvas.toDataURL("image/png");
    if (dataUrl.indexOf("data:image/png") !== 0) {
      throw new Error("canvas did not produce a PNG data URL");
    }
    rasterCache.set(cacheKey, dataUrl);
    return dataUrl;
  }

  function setImportant(style, name, value) {
    style.setProperty(name, value, "important");
  }

  function applyElementBackground(target, dataUrl) {
    setImportant(target.element.style, "background-image", 'url("' + dataUrl + '")');
    setImportant(target.element.style, "background-size", target.width + "px " + target.height + "px");
    setImportant(target.element.style, "background-position", "0 0");
    setImportant(target.element.style, "background-repeat", "no-repeat");
    setImportant(target.element.style, "background-origin", "border-box");
    setImportant(target.element.style, "background-clip", "border-box");
  }

  function ensurePseudoStyleElement() {
    if (pseudoStyleElement) return pseudoStyleElement;
    pseudoStyleElement = document.createElement("style");
    pseudoStyleElement.setAttribute("data-peitho-pdf-flatten", "pseudo-elements");
    document.head.appendChild(pseudoStyleElement);
    return pseudoStyleElement;
  }

  function applyPseudoBackground(target, dataUrl) {
    var className = "peitho-pdf-flatten-" + nextClassId++;
    var rule =
      "." +
      className +
      target.pseudo +
      ' { background-image: url("' +
      dataUrl +
      '") !important; background-size: ' +
      target.width +
      "px " +
      target.height +
      "px !important; background-position: 0 0 !important; background-repeat: no-repeat !important; background-origin: border-box !important; background-clip: border-box !important; }\n";
    var ruleNode = document.createTextNode(rule);
    ensurePseudoStyleElement().appendChild(ruleNode);
    target.element.classList.add(className);
    try {
      var applied = getComputedStyle(target.element, target.pseudo).backgroundImage;
      if (String(applied || "").indexOf('url("data:') !== 0) {
        throw new Error("injected pseudo-element rule did not win the cascade");
      }
    } catch (err) {
      target.element.classList.remove(className);
      if (ruleNode.parentNode) ruleNode.parentNode.removeChild(ruleNode);
      throw err;
    }
  }

  function describeElement(element) {
    var tag = element.tagName ? element.tagName.toLowerCase() : "element";
    var id = element.id ? "#" + element.id : "";
    var classes = "";
    if (typeof element.className === "string" && element.className.trim() !== "") {
      classes =
        "." +
        element.className
          .trim()
          .split(/\s+/)
          .slice(0, 3)
          .join(".");
    }
    return tag + id + classes;
  }

  function describeTarget(target) {
    return target.kind === "pseudo"
      ? describeElement(target.element) + target.pseudo
      : describeElement(target.element);
  }

  function waitForWindowLoad() {
    if (document.readyState === "complete") return Promise.resolve();
    return new Promise(function (resolve) {
      window.addEventListener("load", resolve, { once: true });
    });
  }

  async function waitForStableLayout() {
    if (document.fonts && document.fonts.ready) {
      await document.fonts.ready;
    }
    await waitForWindowLoad();
  }

  async function flattenGradients() {
    var flattened = 0;
    await waitForStableLayout();
    var targets = collectTargets();
    for (var i = 0; i < targets.length; i++) {
      var target = targets[i];
      try {
        var dataUrl = await rasterizeBackground(target.computed, target.width, target.height);
        if (target.kind === "pseudo") {
          applyPseudoBackground(target, dataUrl);
        } else {
          applyElementBackground(target, dataUrl);
        }
        flattened++;
      } catch (err) {
        console.error("peitho pdf flatten:", describeTarget(target), err);
        // Slow but correct: leave the original vector gradient untouched.
      }
    }
    document.documentElement.setAttribute("data-peitho-pdf-flattened", String(flattened));
    return flattened;
  }

  flattenGradients().catch(function (err) {
    console.error("peitho pdf flatten: top-level failure", err);
    document.documentElement.setAttribute("data-peitho-pdf-flattened", "0");
  });
})();
