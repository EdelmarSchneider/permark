// TrailMark landing page: progressive enhancements only. The page works fully without JS.
(function () {
  // 1. OS-aware label on the hero download button (link still points to the latest release).
  var label = document.getElementById("download-label");
  if (label) {
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
    if (os) label.textContent = "Download for " + os;
  }

  // 2. Copy buttons on code blocks.
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
