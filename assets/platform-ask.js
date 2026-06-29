// "Ask the platform" (M26): a natural-language question over the whole catalog.
// Posts to /api/platform/ask and renders the grounded answer as Markdown.
(function ($) {
  "use strict";

  function render(text) {
    var $a = $("#platform-ask-answer");
    if (typeof marked !== "undefined") {
      var $html = $("<div>").html(marked.parse(text || ""));
      $html.find("script, style, iframe").remove();
      $a.empty().append($html);
    } else {
      $a.text(text || "");
    }
  }

  function ask() {
    var q = ($("#platform-ask-input").val() || "").trim();
    if (!q) return;
    var $btn = $("#platform-ask-btn").prop("disabled", true);
    $("#platform-ask-answer").text("Thinking…");
    $.ajax({
      url: "/api/platform/ask", method: "POST", contentType: "application/json",
      data: JSON.stringify({ question: q }),
    })
      .done(function (d) { render(d.answer || "(no answer)"); })
      .fail(function (xhr) {
        var err = xhr.responseJSON && xhr.responseJSON.error;
        $("#platform-ask-answer").text("Error: " + ((err && err.message) || "could not answer"));
      })
      .always(function () { $btn.prop("disabled", false); });
  }

  $(function () {
    $("#platform-ask-btn").on("click", ask);
    $("#platform-ask-input").on("keydown", function (e) {
      if (e.key === "Enter") ask();
    });
  });
})(jQuery);
