// Dashboard trend/distribution/scatter/treemap charts (M35), drawn with the
// vendored MiniChart from the metric series/distribution/portfolio endpoints.
(function ($) {
  "use strict";

  function load(url, cb) {
    $.ajax({ url: url, dataType: "json" }).done(cb).fail(function () {});
  }

  // Per-application sparklines (app detail): one mini trend + latest/delta.
  function appSparklines(MC) {
    var holder = document.getElementById("metric-sparklines");
    if (!holder) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    [["coverage_pct", "Coverage"], ["complexity_avg", "Complexity"], ["loc", "LOC"]].forEach(function (pair) {
      var id = "spark-" + pair[0];
      var row = $('<div class="flex items-center gap-2 py-1 text-sm">')
        .append($('<span class="w-24 text-slate-600">').text(pair[1]))
        .append($('<span>').attr("id", id))
        .append($('<span class="text-xs text-slate-500">').attr("id", id + "-v"));
      $(holder).append(row);
      load("/api/platform/applications/" + m[1] + "/series?metric=" + pair[0], function (d) {
        MC.sparkline("#" + id, (d.series || []).map(function (p) { return p.value; }));
        if (d.latest != null) {
          var delta = d.delta == null ? "" : (d.delta >= 0 ? " ▲" : " ▼") + Math.abs(d.delta).toFixed(1);
          $("#" + id + "-v").text(d.latest.toFixed(1) + delta);
        }
      });
    });
  }

  $(function () {
    if (!global().MiniChart) { return; }
    var MC = global().MiniChart;
    appSparklines(MC);

    load("/api/platform/series?metric=coverage_pct", function (d) { MC.line("#trend-coverage", d.series, { stroke: "#16a34a" }); });
    load("/api/platform/series?metric=complexity_avg", function (d) { MC.line("#trend-complexity", d.series, { stroke: "#dc2626" }); });
    load("/api/platform/distribution?metric=coverage_pct&buckets=10", function (d) { MC.histogram("#dist-coverage", d.buckets, { fill: "#16a34a" }); });

    load("/api/platform/portfolio", function (d) {
      var apps = (d.apps || []).filter(function (a) { return a.loc != null; });
      MC.scatter("#scatter-portfolio", apps.filter(function (a) { return a.coverage_pct != null && a.complexity_avg != null; }).map(function (a) {
        return { x: a.coverage_pct, y: a.complexity_avg, size: a.loc, label: a.name, href: a.href };
      }));
      MC.treemap("#treemap-portfolio", apps.map(function (a) {
        return { size: a.loc, value: a.coverage_pct == null ? null : a.coverage_pct / 100, label: a.name, href: a.href };
      }));
    });
  });

  function global() { return window; }
})(jQuery);
