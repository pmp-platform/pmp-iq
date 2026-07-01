// Settings → Extraction prompts (M34): edit each per-section analyzer/metrics
// prompt template, toggle it, and reset to the shipped default.
(function ($) {
  "use strict";

  function load() {
    $.ajax({ url: "/api/settings/extraction-prompts", dataType: "json" })
      .done(function (d) { render(d.sections || []); })
      .fail(function () { $("#prompts-root").text("Could not load prompts."); });
  }

  function render(sections) {
    var $root = $("#prompts-root").empty();
    sections.forEach(function (s) { $root.append(card(s)); });
  }

  function card(s) {
    var $area = $('<textarea rows="6" class="w-full border border-slate-200 rounded p-2 text-xs font-mono">').val(s.template);
    var $enabled = $('<input type="checkbox">').prop("checked", s.enabled);
    var $msg = $('<span class="text-xs ml-2">');
    var placeholders = s.required_placeholders && s.required_placeholders.length
      ? "Required: " + s.required_placeholders.join(", ") : "";

    var $save = $('<button class="btn btn-primary btn-sm">').text("Save");
    $save.on("click", function () {
      $.ajax({
        url: "/api/settings/extraction-prompts/" + encodeURIComponent(s.section),
        method: "PUT", contentType: "application/json",
        data: JSON.stringify({ template: $area.val(), enabled: $enabled.is(":checked") }),
      })
        .done(function () { $msg.text("Saved").removeClass("text-red-600").addClass("text-green-600"); })
        .fail(function (x) { $msg.text(x.responseJSON && x.responseJSON.error ? x.responseJSON.error.message : "Save failed").removeClass("text-green-600").addClass("text-red-600"); });
    });

    var $reset = $('<button class="text-xs text-slate-600 hover:underline ml-2">').text("Reset to default");
    $reset.on("click", function () {
      $.ajax({ url: "/api/settings/extraction-prompts/" + encodeURIComponent(s.section) + "/reset", method: "POST" })
        .done(load);
    });

    return $('<div class="bg-white rounded-lg shadow border border-slate-200 p-3">')
      .append($('<div class="flex items-center justify-between mb-1">')
        .append($('<span class="font-semibold text-sm">').text(s.section + (s.overridden ? " (edited)" : "")))
        .append($('<label class="text-xs flex items-center gap-1">').append($enabled).append("enabled")))
      .append($area)
      .append($('<div class="flex items-center mt-1">').append($save).append($reset).append($msg))
      .append(placeholders ? $('<div class="text-xs text-slate-400 mt-1">').text(placeholders) : null);
  }

  $(load);
})(jQuery);
