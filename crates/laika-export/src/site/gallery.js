// Laika gallery viewer: keyboard, touch, and screen-reader friendly. The
// page works without it (tiles link straight to the large image).
(function () {
  "use strict";
  var links = Array.prototype.slice.call(document.querySelectorAll("a.ph[data-full]"));
  var box = document.getElementById("lb");
  if (!links.length || !box) return;
  var img = box.querySelector(".lb-stage img");
  var cap = box.querySelector(".lb-cap");
  var count = box.querySelector(".lb-count");
  var prev = box.querySelector(".lb-prev");
  var next = box.querySelector(".lb-next");
  var close = box.querySelector(".lb-close");
  var index = -1;
  var opener = null;

  function show(i) {
    index = (i + links.length) % links.length;
    var a = links[index];
    var thumb = a.querySelector("img");
    img.src = a.getAttribute("data-full");
    img.alt = thumb ? thumb.alt : "";
    cap.textContent = a.getAttribute("data-caption") || "";
    count.textContent = (index + 1) + " / " + links.length;
    var single = links.length < 2;
    prev.hidden = single;
    next.hidden = single;
  }

  function open(i, from) {
    opener = from || null;
    box.hidden = false;
    document.documentElement.style.overflow = "hidden";
    show(i);
    close.focus();
  }

  function shut() {
    box.hidden = true;
    img.removeAttribute("src");
    document.documentElement.style.overflow = "";
    if (opener) opener.focus();
  }

  links.forEach(function (a, i) {
    a.addEventListener("click", function (e) {
      if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
      e.preventDefault();
      open(i, a);
    });
  });
  prev.addEventListener("click", function () { show(index - 1); });
  next.addEventListener("click", function () { show(index + 1); });
  close.addEventListener("click", shut);

  document.addEventListener("keydown", function (e) {
    if (box.hidden) return;
    if (e.key === "Escape") { e.preventDefault(); shut(); }
    else if (e.key === "ArrowLeft") { e.preventDefault(); show(index - 1); }
    else if (e.key === "ArrowRight") { e.preventDefault(); show(index + 1); }
    else if (e.key === "Tab") {
      // Keep focus inside the viewer.
      var f = Array.prototype.filter.call(box.querySelectorAll("button"), function (b) { return !b.hidden; });
      if (!f.length) return;
      var first = f[0], last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
      else if (f.indexOf(document.activeElement) < 0) { e.preventDefault(); first.focus(); }
    }
  });

  var startX = null;
  var stage = box.querySelector(".lb-stage");
  stage.addEventListener("pointerdown", function (e) {
    if (e.pointerType !== "mouse") startX = e.clientX;
  });
  stage.addEventListener("pointerup", function (e) {
    if (startX === null) return;
    var dx = e.clientX - startX;
    startX = null;
    if (Math.abs(dx) > 50) show(index + (dx < 0 ? 1 : -1));
  });
  stage.addEventListener("pointercancel", function () { startX = null; });
})();
