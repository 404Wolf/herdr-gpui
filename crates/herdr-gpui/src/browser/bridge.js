// Herdr UI's page bridge, run before a local page's own scripts. A local
// page is one the agent wrote, so it may hand the agent a result, such as
// the variant the user picked in a mockup:
//
//   herdr.send("Picked variant B", { variant: "B" });
//
// The app only queues it as a note in the page's notes panel: nothing
// reaches the agent until the user presses Send there.
(function () {
  "use strict";
  // The script stays with the view wherever it navigates; only the local
  // page's own scheme gets the bridge.
  var local = /^(herdr-preview:|https?:\/\/herdr-preview\.localhost[/:])/;
  if (!local.test(String(location.href)) || window.herdr) {
    return;
  }
  if (!window.ipc || typeof window.ipc.postMessage !== "function") {
    return;
  }
  var post = window.ipc.postMessage.bind(window.ipc);
  Object.defineProperty(window, "herdr", {
    value: Object.freeze({
      available: true,
      send: function (summary, data) {
        var message = { kind: "send", summary: String(summary == null ? "" : summary) };
        if (data !== undefined) {
          message.data = data;
        }
        try {
          post(JSON.stringify(message));
          return true;
        } catch (_) {
          return false;
        }
      },
    }),
  });
})();
