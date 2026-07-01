// Semantic search, "similar applications", and "possible duplicates" (M40).
// One file drives all three; each block activates only if its element exists.
(function ($) {
  "use strict";

  function link(item) {
    var label = item.name || item.entity_id;
    var score = (item.score === null || item.score === undefined) ? "" :
      " (" + Number(item.score).toFixed(2) + ")";
    var $el = item.href ? $("<a>").attr("href", item.href).addClass("text-blue-600 hover:underline") : $("<span>");
    return $el.text(label + score);
  }

  // --- Global semantic search box (graph page) ---
  function wireSearch() {
    var $input = $("#sem-search-input");
    if (!$input.length) { return; }
    function run() {
      var q = $input.val();
      var $out = $("#sem-search-results").empty();
      if (!q.trim()) { return; }
      $.ajax({ url: "/api/platform/search", data: { q: q }, dataType: "json" })
        .done(function (d) {
          $out.append($('<div class="text-xs text-slate-400 mb-1">').text(d.mode + " match"));
          (d.results || []).forEach(function (r) {
            $out.append($('<div class="py-0.5 text-sm border-b border-slate-100">').append(link(r)
              .after($('<span class="text-xs text-slate-400 ml-1">').text(r.entity_type))));
          });
          if (!(d.results || []).length) { $out.append($('<div class="text-xs text-slate-400">').text("No matches.")); }
        })
        .fail(function () { $out.text("Search failed."); });
    }
    $("#sem-search-btn").on("click", run);
    $input.on("keydown", function (e) { if (e.key === "Enter") { run(); } });
  }

  // --- Similar applications (application detail) ---
  function wireSimilar() {
    var $panel = $("#similar-apps");
    if (!$panel.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    $.ajax({ url: "/api/platform/applications/" + m[1] + "/similar", dataType: "json" })
      .done(function (d) {
        if (!d.enabled) { $panel.html('<div class="text-xs text-slate-400">Embeddings are not configured.</div>'); return; }
        var $list = $panel.empty();
        if (!(d.results || []).length) { $list.append($('<div class="text-xs text-slate-400">').text("No similar applications yet.")); }
        (d.results || []).forEach(function (r) {
          $list.append($('<div class="py-0.5 text-sm">').append(link(r)));
        });
      })
      .fail(function () { $panel.text("Could not load similar applications."); });
  }

  // --- Possible duplicates (dashboard) ---
  function wireDuplicates() {
    var $panel = $("#dup-panel");
    if (!$panel.length) { return; }
    $.ajax({ url: "/api/platform/duplicates", dataType: "json" })
      .done(function (d) {
        if (!d.enabled) { $panel.html('<div class="text-xs text-slate-400">Embeddings are not configured.</div>'); return; }
        var $out = $panel.empty();
        if (!(d.clusters || []).length) { $out.append($('<div class="text-xs text-slate-400">').text("No likely duplicates found.")); }
        (d.clusters || []).forEach(function (c) {
          var $row = $('<div class="py-1 border-b border-slate-100 text-sm flex flex-wrap gap-2">');
          (c.members || []).forEach(function (m2) { $row.append(link(m2)); });
          $out.append($row);
        });
      })
      .fail(function () { $panel.text("Could not load duplicates."); });
  }

  $(function () {
    wireSearch();
    wireSimilar();
    wireDuplicates();
  });
})(jQuery);
