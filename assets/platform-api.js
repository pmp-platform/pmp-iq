// API endpoints panel (M42): the operations this application exposes, grouped by
// protocol, each with the applications that consume it (impact).
(function ($) {
  "use strict";

  $(function () {
    var $panel = $("#api-endpoints");
    if (!$panel.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }

    $.ajax({ url: "/api/platform/applications/" + m[1] + "/endpoints", dataType: "json" })
      .done(function (d) {
        var rows = d.endpoints || [];
        $panel.empty();
        if (!rows.length) { $panel.append($('<div class="text-xs text-slate-400">').text("No API endpoints detected.")); return; }
        rows.forEach(function (r) {
          var ep = r.endpoint;
          var consumers = (r.consumers || []).map(function (c) { return c.name; });
          var $row = $('<div class="py-1 border-b border-slate-100">')
            .append($('<span class="inline-block text-xs font-mono bg-slate-100 rounded px-1 mr-2">').text(ep.protocol))
            .append($('<span class="font-mono">').text(ep.operation));
          if (ep.summary) { $row.append($('<span class="text-slate-500 ml-2">').text(ep.summary)); }
          if (consumers.length) {
            $row.append($('<div class="text-xs text-slate-500 mt-0.5">').text("consumed by: " + consumers.join(", ")));
          }
          $panel.append($row);
        });
      })
      .fail(function () { $panel.text("Could not load API endpoints."); });
  });
})(jQuery);
