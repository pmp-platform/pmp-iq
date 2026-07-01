// C4 model page (M29 + M38): renders the C4 Mermaid diagram and Structurizr DSL
// for the selected level (Context / Container / Component), fetched from
// /api/platform/c4. Container/Component require an application; the drop-down is
// populated from /api/platform/applications.
(function ($) {
  "use strict";

  var apps = [];

  function level() { return $("#c4-level").val() || "context"; }
  function needsApp() { return level() === "container" || level() === "component"; }

  // Load the application list once, for the Container/Component picker.
  function loadApps() {
    return $.ajax({ url: "/api/platform/applications", data: { page_size: 200 }, dataType: "json" })
      .done(function (d) {
        apps = (d && d.items) || [];
        var $sel = $("#c4-app");
        $sel.find("option:not(:first)").remove();
        apps.forEach(function (a) {
          $sel.append($("<option>").val(a.id).text(a.name));
        });
      });
  }

  // Fetch + render the C4 model for the current level/application.
  function load() {
    var lvl = level();
    var appId = $("#c4-app").val();
    $("#c4-app").toggleClass("hidden", !needsApp());
    if (needsApp() && !appId) {
      $("#c4-diagram").text("Select an application to view its " + lvl + " diagram.");
      $("#c4-dsl").val("");
      return;
    }
    var params = { level: lvl, dependencies: $("#c4-deps").is(":checked") };
    if (needsApp()) { params.application = appId; }
    $("#c4-diagram").text("Loading…");
    $.ajax({ url: "/api/platform/c4", data: params, dataType: "json" })
      .done(function (d) {
        $("#c4-dsl").val(d.dsl || "");
        if (typeof mermaid === "undefined") { $("#c4-diagram").text("Mermaid is unavailable."); return; }
        mermaid.render("c4svg", d.mermaid || "C4Context")
          .then(function (res) { $("#c4-diagram").html(res.svg); })
          .catch(function () { $("#c4-diagram").text("Could not render the C4 diagram."); });
      })
      .fail(function () { $("#c4-diagram").text("Could not load the C4 model."); });
  }

  $(function () {
    if (typeof mermaid !== "undefined") {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
    }
    $("#c4-level, #c4-app, #c4-deps").on("change", load);
    loadApps().always(load);

    $("#c4-copy").on("click", function () {
      var ta = document.getElementById("c4-dsl");
      ta.select();
      try { document.execCommand("copy"); } catch (e) { /* ignore */ }
    });
  });
})(jQuery);
