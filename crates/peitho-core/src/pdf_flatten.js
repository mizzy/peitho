(function () {
  var PSEUDOS = ["::before", "::after"];
  var SCALE = 2;
  var MAX_CANVAS_DIMENSION = 16384;
  var MAX_CANVAS_AREA = 268000000;
  var FONT_READY_TIMEOUT_MS = 2000; // Below Chrome --virtual-time-budget=10000.
  var nextClassId = 0;
  var pseudoStyleElement = null;
  var rasterCache = new Map();
  var shadowRasterCache = new Map();

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

  function splitCssList(value) {
    var parts = [];
    var current = "";
    var depth = 0;
    var quote = null;
    var escaped = false;
    var text = String(value || "");
    for (var i = 0; i < text.length; i++) {
      var ch = text[i];
      current += ch;
      if (quote) {
        if (escaped) {
          escaped = false;
        } else if (ch === "\\") {
          escaped = true;
        } else if (ch === quote) {
          quote = null;
        }
        continue;
      }
      if (ch === '"' || ch === "'") {
        quote = ch;
      } else if (ch === "(") {
        depth++;
      } else if (ch === ")") {
        depth--;
        if (depth < 0) return null;
      } else if (ch === "," && depth === 0) {
        parts.push(current.slice(0, -1).trim());
        current = "";
      }
    }
    if (quote || depth !== 0) return null;
    parts.push(current.trim());
    return parts;
  }

  function splitCssWhitespace(value) {
    var parts = [];
    var current = "";
    var depth = 0;
    var quote = null;
    var escaped = false;
    var text = String(value || "").trim();
    for (var i = 0; i < text.length; i++) {
      var ch = text[i];
      if (quote) {
        current += ch;
        if (escaped) {
          escaped = false;
        } else if (ch === "\\") {
          escaped = true;
        } else if (ch === quote) {
          quote = null;
        }
        continue;
      }
      if (ch === '"' || ch === "'") {
        quote = ch;
        current += ch;
      } else if (ch === "(") {
        depth++;
        current += ch;
      } else if (ch === ")") {
        depth--;
        if (depth < 0) return null;
        current += ch;
      } else if (/\s/.test(ch) && depth === 0) {
        if (current.trim() !== "") {
          parts.push(current.trim());
          current = "";
        }
      } else {
        current += ch;
      }
    }
    if (quote || depth !== 0) return null;
    if (current.trim() !== "") parts.push(current.trim());
    return parts;
  }

  function parsePixel(value) {
    var match = String(value || "").match(/^([0-9]+(?:\.[0-9]+)?)px$/);
    if (!match) return null;
    var parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function parseSignedPixel(value) {
    var match = String(value || "").match(/^(-?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+))px$/);
    if (!match) return null;
    var parsed = Number(match[1]);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function ensureCanvasSize(width, height, label) {
    if (
      width > MAX_CANVAS_DIMENSION ||
      height > MAX_CANVAS_DIMENSION ||
      width * height > MAX_CANVAS_AREA
    ) {
      throw new Error("canvas too large for pdf " + label + " flatten: " + width + "x" + height);
    }
  }

  function createCanvas(width, height, label) {
    var canvasWidth = Math.ceil(width);
    var canvasHeight = Math.ceil(height);
    ensureCanvasSize(canvasWidth, canvasHeight, label);
    var canvas = document.createElement("canvas");
    canvas.width = canvasWidth;
    canvas.height = canvasHeight;
    var context = canvas.getContext("2d");
    if (!context) throw new Error("2d canvas context unavailable");
    return { canvas: canvas, context: context };
  }

  function canvasPngDataUrl(canvas) {
    var dataUrl = canvas.toDataURL("image/png");
    if (dataUrl.indexOf("data:image/png") !== 0) {
      throw new Error("canvas did not produce a PNG data URL");
    }
    return dataUrl;
  }

  function scaleRect(rect, scale) {
    return {
      x: rect.x * scale,
      y: rect.y * scale,
      width: rect.width * scale,
      height: rect.height * scale,
    };
  }

  function inflateRect(rect, amount) {
    return {
      x: rect.x - amount,
      y: rect.y - amount,
      width: rect.width + amount * 2,
      height: rect.height + amount * 2,
    };
  }

  function deflateRect(rect, amount) {
    return inflateRect(rect, -amount);
  }

  function scaleRadii(radii, scale) {
    return {
      topLeft: radii.topLeft * scale,
      topRight: radii.topRight * scale,
      bottomRight: radii.bottomRight * scale,
      bottomLeft: radii.bottomLeft * scale,
    };
  }

  function adjustRadii(radii, amount) {
    return {
      topLeft: Math.max(0, radii.topLeft + amount),
      topRight: Math.max(0, radii.topRight + amount),
      bottomRight: Math.max(0, radii.bottomRight + amount),
      bottomLeft: Math.max(0, radii.bottomLeft + amount),
    };
  }

  function clampRadius(radius, width, height) {
    return Math.max(0, Math.min(radius, width / 2, height / 2));
  }

  function appendRoundedRectPath(context, rect, radii) {
    if (rect.width <= 0 || rect.height <= 0) {
      throw new Error("shadow rectangle collapsed");
    }
    var topLeft = clampRadius(radii.topLeft, rect.width, rect.height);
    var topRight = clampRadius(radii.topRight, rect.width, rect.height);
    var bottomRight = clampRadius(radii.bottomRight, rect.width, rect.height);
    var bottomLeft = clampRadius(radii.bottomLeft, rect.width, rect.height);
    var right = rect.x + rect.width;
    var bottom = rect.y + rect.height;
    context.moveTo(rect.x + topLeft, rect.y);
    context.lineTo(right - topRight, rect.y);
    context.quadraticCurveTo(right, rect.y, right, rect.y + topRight);
    context.lineTo(right, bottom - bottomRight);
    context.quadraticCurveTo(right, bottom, right - bottomRight, bottom);
    context.lineTo(rect.x + bottomLeft, bottom);
    context.quadraticCurveTo(rect.x, bottom, rect.x, bottom - bottomLeft);
    context.lineTo(rect.x, rect.y + topLeft);
    context.quadraticCurveTo(rect.x, rect.y, rect.x + topLeft, rect.y);
  }

  function fillRoundedRect(context, rect, radii) {
    context.beginPath();
    appendRoundedRectPath(context, rect, radii);
    context.closePath();
    context.fill();
  }

  function clipRoundedRect(context, rect, radii) {
    context.beginPath();
    appendRoundedRectPath(context, rect, radii);
    context.closePath();
    context.clip();
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
      backgroundAttachment: computed.backgroundAttachment,
      backgroundBlendMode: computed.backgroundBlendMode,
      boxShadow: computed.boxShadow,
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
      borderTopLeftRadius: computed.borderTopLeftRadius,
      borderTopRightRadius: computed.borderTopRightRadius,
      borderBottomRightRadius: computed.borderBottomRightRadius,
      borderBottomLeftRadius: computed.borderBottomLeftRadius,
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

  function collectGradientTargets() {
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
    var raster = createCanvas(canvasWidth, canvasHeight, "gradient");
    var canvas = raster.canvas;
    var context = raster.context;
    context.scale(SCALE, SCALE);
    context.drawImage(image, 0, 0, width, height);
    var dataUrl = canvasPngDataUrl(canvas);
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

  function parseShadowList(boxShadow) {
    var value = String(boxShadow || "").trim();
    if (value === "" || value.toLowerCase() === "none") return [];
    var layers = splitCssList(value);
    if (layers === null) throw new Error("unsupported box-shadow list syntax");
    return layers.map(parseShadowLayer);
  }

  function parseShadowLayer(layer) {
    var tokens = splitCssWhitespace(layer);
    if (tokens === null || tokens.length === 0) throw new Error("unsupported box-shadow syntax");

    var inset = false;
    var filtered = [];
    tokens.forEach(function (token) {
      if (token.toLowerCase() === "inset") {
        inset = true;
      } else {
        filtered.push(token);
      }
    });

    if (filtered.length < 3 || !/^rgba?\(/i.test(filtered[0])) {
      throw new Error("unsupported box-shadow color syntax");
    }

    var color = filtered[0];
    var lengths = filtered.slice(1).map(function (token) {
      var parsed = parseSignedPixel(token);
      if (parsed === null) throw new Error("unsupported box-shadow length: " + token);
      return parsed;
    });

    if (lengths.length < 2 || lengths.length > 4) {
      throw new Error("unsupported box-shadow length count");
    }

    var blur = lengths.length >= 3 ? lengths[2] : 0;
    if (blur < 0) throw new Error("box-shadow blur cannot be negative");

    return {
      inset: inset,
      color: color,
      offsetX: lengths[0],
      offsetY: lengths[1],
      blur: blur,
      spread: lengths.length >= 4 ? lengths[3] : 0,
    };
  }

  function parseCornerRadius(value) {
    var text = String(value || "").trim();
    if (/\s/.test(text)) return null;
    return parsePixel(text);
  }

  function parseBorderRadii(computed) {
    var radii = {
      topLeft: parseCornerRadius(computed.borderTopLeftRadius),
      topRight: parseCornerRadius(computed.borderTopRightRadius),
      bottomRight: parseCornerRadius(computed.borderBottomRightRadius),
      bottomLeft: parseCornerRadius(computed.borderBottomLeftRadius),
    };
    if (
      radii.topLeft === null ||
      radii.topRight === null ||
      radii.bottomRight === null ||
      radii.bottomLeft === null
    ) {
      throw new Error("unsupported border-radius for box-shadow flatten");
    }
    return radii;
  }

  function hasTransformBeforeSlide(element, slide) {
    for (var node = element; node && node !== slide; node = node.parentElement) {
      if (getComputedStyle(node).transform !== "none") return true;
    }
    return false;
  }

  function splitShadows(shadows, inset) {
    return shadows.filter(function (shadow) {
      return shadow.inset === inset;
    });
  }

  function maxShadowPadding(shadows, axis) {
    var max = 0;
    shadows.forEach(function (shadow) {
      var offset = axis === "x" ? shadow.offsetX : shadow.offsetY;
      max = Math.max(max, shadow.blur * 2 + Math.abs(offset) + Math.max(0, shadow.spread));
    });
    return Math.ceil(max);
  }

  function collectShadowTargets() {
    var targets = [];
    var elements = document.querySelectorAll("*");
    elements.forEach(function (element) {
      try {
        var computed = getComputedStyle(element);
        var shadows = parseShadowList(computed.boxShadow);
        if (shadows.length === 0) return;

        var slide = element.closest(".peitho-slide");
        if (!slide) throw new Error("element is outside .peitho-slide");
        if (hasTransformBeforeSlide(element, slide)) throw new Error("transformed element ancestor");
        if (element.getClientRects().length !== 1) throw new Error("fragmented element");

        var slideRect = slide.getBoundingClientRect();
        var elementRect = element.getBoundingClientRect();
        var scale = slideRect.width / slide.offsetWidth;
        if (!Number.isFinite(scale) || scale <= 0) throw new Error("invalid slide scale");

        var width = elementRect.width / scale;
        var height = elementRect.height / scale;
        if (width <= 0 || height <= 0) throw new Error("zero-sized element");

        var outerShadows = splitShadows(shadows, false);
        var insetShadows = splitShadows(shadows, true);
        if (insetShadows.length > 0) validateInsetBackground(computed);

        targets.push({
          element: element,
          slide: slide,
          computed: snapshotComputed(computed),
          shadows: shadows,
          outerShadows: outerShadows,
          insetShadows: insetShadows,
          localX: (elementRect.left - slideRect.left) / scale - slide.clientLeft,
          localY: (elementRect.top - slideRect.top) / scale - slide.clientTop,
          width: width,
          height: height,
          radii: parseBorderRadii(computed),
        });
      } catch (err) {
        console.error("peitho pdf shadow flatten:", describeElement(element), err);
      }
    });
    return targets;
  }

  function shadowCacheKey(kind, target, shadows) {
    return JSON.stringify({
      kind: kind,
      width: target.width,
      height: target.height,
      radii: target.radii,
      shadows: shadows,
    });
  }

  function rasterizeOuterShadows(target) {
    var shadows = target.outerShadows;
    if (shadows.length === 0) return null;

    var cacheKey = shadowCacheKey("outer", target, shadows);
    if (shadowRasterCache.has(cacheKey)) return shadowRasterCache.get(cacheKey);

    var padX = maxShadowPadding(shadows, "x");
    var padY = maxShadowPadding(shadows, "y");
    var cssWidth = target.width + padX * 2;
    var cssHeight = target.height + padY * 2;
    var raster = createCanvas(cssWidth * SCALE, cssHeight * SCALE, "box-shadow");
    var context = raster.context;
    var baseRect = { x: padX, y: padY, width: target.width, height: target.height };
    var farAway = cssWidth + cssHeight + 1024;
    var scaledFarAway = farAway * SCALE;

    context.fillStyle = "#000";
    shadows
      .slice()
      .reverse()
      .forEach(function (shadow) {
        var inflated = inflateRect(baseRect, shadow.spread);
        if (inflated.width <= 0 || inflated.height <= 0) {
          throw new Error("outer box-shadow spread collapses shape");
        }
        var scaledRect = scaleRect(inflated, SCALE);
        var scaledRadii = scaleRadii(adjustRadii(target.radii, shadow.spread), SCALE);
        context.shadowColor = shadow.color;
        context.shadowBlur = shadow.blur * SCALE;
        context.shadowOffsetX = shadow.offsetX * SCALE + scaledFarAway;
        context.shadowOffsetY = shadow.offsetY * SCALE;
        fillRoundedRect(
          context,
          {
            x: scaledRect.x - scaledFarAway,
            y: scaledRect.y,
            width: scaledRect.width,
            height: scaledRect.height,
          },
          scaledRadii
        );
      });

    var result = {
      dataUrl: canvasPngDataUrl(raster.canvas),
      padX: padX,
      padY: padY,
      cssWidth: cssWidth,
      cssHeight: cssHeight,
    };
    shadowRasterCache.set(cacheKey, result);
    return result;
  }

  function maxInsetExtent(shadows, width, height) {
    var max = Math.max(width, height);
    shadows.forEach(function (shadow) {
      max = Math.max(
        max,
        shadow.blur * 2 + Math.abs(shadow.offsetX) + Math.abs(shadow.offsetY) + Math.abs(shadow.spread)
      );
    });
    return Math.ceil(max + 1024);
  }

  function rasterizeInsetShadows(target) {
    var shadows = target.insetShadows;
    if (shadows.length === 0) return null;

    var cacheKey = shadowCacheKey("inset", target, shadows);
    if (shadowRasterCache.has(cacheKey)) return shadowRasterCache.get(cacheKey);

    var raster = createCanvas(target.width * SCALE, target.height * SCALE, "box-shadow");
    var context = raster.context;
    var borderBox = { x: 0, y: 0, width: target.width, height: target.height };
    var huge = maxInsetExtent(shadows, target.width, target.height);

    context.save();
    clipRoundedRect(context, scaleRect(borderBox, SCALE), scaleRadii(target.radii, SCALE));
    shadows
      .slice()
      .reverse()
      .forEach(function (shadow) {
        var inner = deflateRect(borderBox, shadow.spread);
        if (inner.width <= 0 || inner.height <= 0) {
          throw new Error("inset box-shadow spread collapses shape");
        }
        context.fillStyle = shadow.color;
        context.shadowColor = shadow.color;
        context.shadowBlur = shadow.blur * SCALE;
        context.shadowOffsetX = shadow.offsetX * SCALE;
        context.shadowOffsetY = shadow.offsetY * SCALE;
        context.beginPath();
        context.rect(
          -huge * SCALE,
          -huge * SCALE,
          (target.width + huge * 2) * SCALE,
          (target.height + huge * 2) * SCALE
        );
        appendRoundedRectPath(
          context,
          scaleRect(inner, SCALE),
          scaleRadii(adjustRadii(target.radii, -shadow.spread), SCALE)
        );
        context.fill("evenodd");
      });
    context.restore();

    var dataUrl = canvasPngDataUrl(raster.canvas);
    shadowRasterCache.set(cacheKey, dataUrl);
    return dataUrl;
  }

  function validateInsetBackground(computed) {
    if (hasFixedAttachment(computed)) throw new Error("background-attachment: fixed is unsupported");
    if (hasNonNormalBlendMode(computed)) throw new Error("background-blend-mode is unsupported");
    [
      computed.backgroundImage,
      computed.backgroundSize,
      computed.backgroundPosition,
      computed.backgroundRepeat,
      computed.backgroundOrigin,
      computed.backgroundClip,
    ].forEach(function (value) {
      if (splitCssList(value) === null) throw new Error("unsupported background layer list");
    });
  }

  function prependBackgroundValue(oldValue, newValue, includeTail) {
    return includeTail ? newValue + ", " + oldValue : newValue;
  }

  function applyInsetBackground(target, dataUrl) {
    var computed = target.computed;
    validateInsetBackground(computed);
    var hasTail = String(computed.backgroundImage || "").trim().toLowerCase() !== "none";
    setImportant(
      target.element.style,
      "background-image",
      prependBackgroundValue(computed.backgroundImage, 'url("' + dataUrl + '")', hasTail)
    );
    setImportant(
      target.element.style,
      "background-size",
      prependBackgroundValue(computed.backgroundSize, target.width + "px " + target.height + "px", hasTail)
    );
    setImportant(
      target.element.style,
      "background-position",
      prependBackgroundValue(computed.backgroundPosition, "0 0", hasTail)
    );
    setImportant(
      target.element.style,
      "background-repeat",
      prependBackgroundValue(computed.backgroundRepeat, "no-repeat", hasTail)
    );
    setImportant(
      target.element.style,
      "background-origin",
      prependBackgroundValue(computed.backgroundOrigin, "border-box", hasTail)
    );
    setImportant(
      target.element.style,
      "background-clip",
      prependBackgroundValue(computed.backgroundClip, "border-box", hasTail)
    );
  }

  async function applyOuterShadow(target, raster) {
    var image = await loadImage(raster.dataUrl);
    image.setAttribute("data-peitho-pdf-shadow", "outer");
    image.style.position = "absolute";
    image.style.left = target.localX - raster.padX + "px";
    image.style.top = target.localY - raster.padY + "px";
    image.style.width = raster.cssWidth + "px";
    image.style.height = raster.cssHeight + "px";
    image.style.zIndex = "-1";
    image.style.pointerEvents = "none";
    image.style.maxWidth = "none";
    image.style.border = "0";
    target.slide.appendChild(image);
    return image;
  }

  async function applyShadowTarget(target) {
    var outerRaster = rasterizeOuterShadows(target);
    var insetDataUrl = rasterizeInsetShadows(target);
    var originalCssText = target.element.style.cssText;
    var addedNodes = [];
    try {
      if (outerRaster !== null) {
        addedNodes.push(await applyOuterShadow(target, outerRaster));
      }
      if (insetDataUrl !== null) {
        applyInsetBackground(target, insetDataUrl);
      }
      setImportant(target.element.style, "box-shadow", "none");
    } catch (err) {
      target.element.style.cssText = originalCssText;
      addedNodes.forEach(function (node) {
        if (node.parentNode) node.parentNode.removeChild(node);
      });
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
      await Promise.race([
        document.fonts.ready.then(function () {}, function () {}),
        new Promise(function (resolve) {
          setTimeout(resolve, FONT_READY_TIMEOUT_MS);
        })
      ]);
    }
    await waitForWindowLoad();
  }

  async function flattenGradients() {
    var flattened = 0;
    var targets = collectGradientTargets();
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
    return flattened;
  }

  async function flattenBoxShadows() {
    var flattened = 0;
    var targets = collectShadowTargets();
    for (var i = 0; i < targets.length; i++) {
      var target = targets[i];
      try {
        await applyShadowTarget(target);
        flattened++;
      } catch (err) {
        console.error("peitho pdf shadow flatten:", describeTarget(target), err);
        // Slow but correct: leave the original CSS box-shadow untouched.
      }
    }
    return flattened;
  }

  async function flattenPdfArtifacts() {
    await waitForStableLayout();
    var gradientCount = await flattenGradients();
    var shadowCount = await flattenBoxShadows();
    document.documentElement.setAttribute("data-peitho-pdf-flattened", String(gradientCount + shadowCount));
    document.documentElement.setAttribute("data-peitho-pdf-shadow-flattened", String(shadowCount));
    return gradientCount + shadowCount;
  }

  flattenPdfArtifacts().catch(function (err) {
    console.error("peitho pdf flatten: top-level failure", err);
    document.documentElement.setAttribute("data-peitho-pdf-flattened", "0");
    document.documentElement.setAttribute("data-peitho-pdf-shadow-flattened", "0");
  });
})();
