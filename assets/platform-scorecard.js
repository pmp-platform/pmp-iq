// Production-readiness scorecard (M43): the per-application checks + level on the
// app detail, and the fleet ranking on the Insights dashboard.
(function ($) {
  "use strict";

  var LEVEL_CLASS = {
    gold: "text-amber-600", silver: "text-slate-500", bronze: "text-orange-700", at_risk: "text-red-600",
  };

  function levelBadge(level) {
    return $('<span class="font-semibold capitalize">').addClass(LEVEL_CLASS[level] || "").text((level || "").replace("_", " "));
  }

  function appScorecard() {
    var $panel = $("#scorecard");
    if (!$panel.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    $.ajax({ url: "/api/platform/applications/" + m[1] + "/scorecard", dataType: "json" })
      .done(function (d) {
        $panel.empty();
        $panel.append($('<div class="mb-2">')
          .append(levelBadge(d.level))
          .append($('<span class="text-slate-500 ml-2">').text("score " + Math.round((d.score || 0) * 100) + "%")));
        (d.results || []).forEach(function (r) {
          $panel.append($('<div class="py-0.5 flex items-center gap-2">')
            .append($("<span>").addClass(r.passed ? "text-green-600" : "text-red-600").text(r.passed ? "✓" : "✗"))
            .append($("<span>").text(r.check_id))
            .append($('<span class="text-xs text-slate-400 ml-auto">').text(r.severity)));
        });
      })
      .fail(function () { $panel.text("Could not load scorecard."); });
  }

  function fleetScorecards() {
    var $panel = $("#fleet-scorecards");
    if (!$panel.length) { return; }
    $.ajax({ url: "/api/platform/scorecards", dataType: "json" })
      .done(function (d) {
        $panel.empty();
        var rows = d.scorecards || [];
        if (!rows.length) { $panel.append($('<div class="text-xs text-slate-400">').text("No applications yet.")); return; }
        rows.forEach(function (r) {
          var $name = r.href ? $("<a>").attr("href", r.href).addClass("text-blue-600 hover:underline").text(r.name)
            : $("<span>").text(r.name);
          $panel.append($('<div class="py-0.5 border-b border-slate-100 flex justify-between text-sm">')
            .append($name)
            .append($("<span>").append(levelBadge(r.level)).append($('<span class="text-slate-400 ml-2">').text(Math.round((r.score || 0) * 100) + "%"))));
        });
      })
      .fail(function () { $panel.text("Could not load scorecards."); });
  }

  $(function () { appScorecard(); fleetScorecards(); });
})(jQuery);
