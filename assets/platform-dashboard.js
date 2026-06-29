// Platform insights dashboard (M32): rollup tiles, leaderboards, and group-by
// breakdowns of the quality metrics, fetched from /api/platform/dashboard.
(function ($) {
  "use strict";

  function tile(label, value) {
    return '<div class="bg-white rounded-lg shadow border border-slate-200 p-3">' +
      '<div class="text-xs text-slate-500">' + label + "</div>" +
      '<div class="text-xl font-bold">' + value + "</div></div>";
  }

  function num(v, suffix) {
    if (v === null || v === undefined) return "—";
    var n = Math.round(v * 10) / 10;
    return n + (suffix || "");
  }

  function nameValueTable(rows, suffix) {
    if (!rows || !rows.length) return '<div class="text-sm text-slate-400">No data yet.</div>';
    var body = rows.map(function (r) {
      return '<tr><td class="py-0.5 pr-4">' + $("<span>").text(r.name).html() + "</td>" +
        '<td class="py-0.5 text-right font-mono">' + num(r.value, suffix) + "</td></tr>";
    }).join("");
    return '<table class="w-full text-sm">' + body + "</table>";
  }

  function groupTable(rows) {
    if (!rows || !rows.length) return '<div class="text-sm text-slate-400">No data yet.</div>';
    var body = rows.map(function (r) {
      return '<tr><td class="py-0.5 pr-4">' + $("<span>").text(r.group).html() + "</td>" +
        '<td class="py-0.5 text-right font-mono">' + num(r.avg, "%") + "</td>" +
        '<td class="py-0.5 text-right text-slate-400">' + r.count + "</td></tr>";
    }).join("");
    return '<table class="w-full text-sm"><tr class="text-xs text-slate-500"><td>Group</td>' +
      '<td class="text-right">Avg coverage</td><td class="text-right">Apps</td></tr>' + body + "</table>";
  }

  $(function () {
    $.ajax({ url: "/api/platform/dashboard", dataType: "json" }).done(function (d) {
      var r = d.rollup || {};
      $("#dash-rollup").html(
        tile("Applications", r.applications || 0) +
        tile("With metrics", r.with_metrics || 0) +
        tile("With CI", r.with_ci || 0) +
        tile("Avg coverage", num(r.avg_coverage, "%")) +
        tile("Avg complexity", num(r.avg_complexity))
      );
      var lb = d.leaderboards || {};
      $("#dash-top-coverage").html(nameValueTable(lb.top_coverage, "%"));
      $("#dash-needs-coverage").html(nameValueTable(lb.needs_coverage, "%"));
      $("#dash-lowest-complexity").html(nameValueTable(lb.lowest_complexity, ""));
      $("#dash-by-language").html(groupTable((d.groups || {}).coverage_by_language));
    }).fail(function () {
      $("#dash-rollup").html('<div class="text-sm text-red-600">Could not load dashboard.</div>');
    });
  });
})(jQuery);
