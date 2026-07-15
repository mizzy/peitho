(function () {
  var CHUNK = "PEITHO_LINT_" + "CHUNK";
  var DONE = "PEITHO_LINT_" + "DONE";
  // Keep each console payload comfortably below macOS PIPE_BUF after Chrome's log wrapper.
  var CHUNK_SIZE = 300;

  function waitForWindowLoad() {
    if (document.readyState === "complete") {
      return Promise.resolve();
    }
    return new Promise(function (resolve) {
      window.addEventListener("load", resolve, { once: true });
    });
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
    return document.fonts.ready;
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

  function measureSlide(slide, index) {
    var slideRect = slide.getBoundingClientRect();
    var bounds = contentBounds(slide, slideRect);

    return {
      slide: index + 1,
      contentWidth: Math.max(bounds.maxRight - bounds.minLeft, slide.scrollWidth),
      contentHeight: Math.max(bounds.maxBottom - bounds.minTop, slide.scrollHeight),
      boxWidth: slideRect.width,
      boxHeight: slideRect.height
    };
  }

  function measureSlides() {
    return Array.prototype.map.call(
      document.querySelectorAll("section.peitho-slide"),
      measureSlide
    );
  }

  function publish(results) {
    var payload = btoa(JSON.stringify(results));
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
