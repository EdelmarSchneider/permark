// TrailMark landing page: progressive enhancements only. The page works fully without JS.
(function () {
  // 1. OS-aware hero button: the Microsoft Store on Windows, the latest release elsewhere.
  var label = document.getElementById("download-label");
  var button = document.getElementById("download-btn");
  if (label && button) {
    var ua =
      (navigator.userAgentData && navigator.userAgentData.platform) ||
      navigator.platform ||
      navigator.userAgent ||
      "";
    var os = /win/i.test(ua)
      ? "Windows"
      : /mac|iphone|ipad/i.test(ua)
        ? "macOS"
        : /linux|x11/i.test(ua) && !/android/i.test(navigator.userAgent)
          ? "Linux"
          : "";
    if (os === "Windows") {
      label.textContent = "Get it from the Microsoft Store";
      button.setAttribute("href", "https://apps.microsoft.com/detail/9NVV5GVZMGGX");
    } else if (os) {
      label.textContent = "Download for " + os;
    }
  }

  // 2. The loop video stays on its poster for people who ask the system for less motion.
  if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    document.querySelectorAll("video[autoplay]").forEach(function (video) {
      video.removeAttribute("autoplay");
      video.pause();
      video.controls = true;
    });
  }

  // 3. Copy buttons on code blocks.
  document.querySelectorAll("[data-copy]").forEach(function (btn) {
    if (!navigator.clipboard) {
      btn.hidden = true;
      return;
    }
    btn.addEventListener("click", function () {
      navigator.clipboard.writeText(btn.getAttribute("data-copy")).then(function () {
        btn.textContent = "Copied";
        btn.setAttribute("data-state", "done");
        setTimeout(function () {
          btn.textContent = "Copy";
          btn.removeAttribute("data-state");
        }, 1600);
      });
    });
  });
})();
