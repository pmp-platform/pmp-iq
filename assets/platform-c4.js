// C4 model page (M29): renders the C4 System-Context Mermaid diagram and shows
// the Structurizr DSL export, both fetched from /api/platform/c4.
(function ($) {
  "use strict";

  $(function () {
    if (typeof mermaid !== "undefined") {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
    }
    $.ajax({ url: "/api/platform/c4", dataType: "json" })
      .done(function (d) {
        $("#c4-dsl").val(d.dsl || "");
        if (typeof mermaid === "undefined") { $("#c4-diagram").text("Mermaid is unavailable."); return; }
        mermaid.render("c4svg", d.mermaid || "C4Context")
          .then(function (res) { $("#c4-diagram").html(res.svg); })
          .catch(function () { $("#c4-diagram").text("Could not render the C4 diagram."); });
      })
      .fail(function () { $("#c4-diagram").text("Could not load the C4 model."); });

    $("#c4-copy").on("click", function () {
      var ta = document.getElementById("c4-dsl");
      ta.select();
      try { document.execCommand("copy"); } catch (e) { /* ignore */ }
    });
  });
})(jQuery);
