// PerMark landing page: progressive enhancements only. The page works fully without JS.
(function () {
  // Store link with the campaign ID (cid): Partner Center counts the visits and installs that came from this site.
  // The same link is in index.html (Install, Windows). Replace "landing-page" in both when the campaign exists.
  var STORE = "https://apps.microsoft.com/detail/9NVV5GVZMGGX?cid=landing-page";
  var RELEASES = "https://github.com/EdelmarSchneider/permark/releases/latest";

  // The visitor's system: "Windows", "macOS", "Linux", or "" when unknown (phones, tablets, anything else).
  function detectOS(nav) {
    var ua = nav.userAgent || "";
    if (/android|iphone|ipad|ipod/i.test(ua)) return "";
    var platform = (nav.userAgentData && nav.userAgentData.platform) || nav.platform || ua;
    if (/mac/i.test(platform)) return nav.maxTouchPoints > 1 ? "" : "macOS"; // iPadOS reports a Mac
    if (/win/i.test(platform)) return "Windows";
    if (/linux|x11/i.test(platform)) return "Linux";
    return "";
  }

  // 1. Download buttons (all of them): the Microsoft Store on Windows, the latest release on macOS and Linux.
  //    The code-signing note shows only where the GitHub downloads apply. Unknown system: the plain link to #install stays.
  var os = detectOS(navigator);
  if (os) {
    document.querySelectorAll("[data-download]").forEach(function (link) {
      var label = link.querySelector("[data-download-label]");
      link.setAttribute("href", os === "Windows" ? STORE : RELEASES);
      if (label) label.textContent = os === "Windows" ? "Get it from the Microsoft Store" : "Download for " + os;
    });
    if (os !== "Windows") {
      document.querySelectorAll("[data-download-note]").forEach(function (note) {
        note.hidden = false;
      });
    }
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
