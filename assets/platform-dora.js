// DORA delivery performance (M47). On the Insights dashboard: the four fleet
// metrics + tier and a per-application table. On an application detail page: a
// single-app DORA panel (#dora-app).
(function ($) {
  "use strict";

  var TIER_COLOR = { elite: "text-green-700", high: "text-blue-700", medium: "text-amber-700", low: "text-red-700" };

  function tierBadge(tier) {
    return $('<span class="font-semibold capitalize">').addClass(TIER_COLOR[tier] || "text-slate-700").text(tier || "—");
  }

  function fmt(v, suffix) {
    if (v === null || v === undefined) { return "—"; }
    return (Math.round(v * 10) / 10) + (suffix || "");
  }

  function metricCard(label, value) {
    return $('<div class="bg-white rounded-lg shadow border border-slate-200 p-3">')
      .append($('<div class="text-xs text-slate-500">').text(label))
      .append($('<div class="text-lg font-bold">').text(value));
  }

  function renderSummary($cards, s) {
    $cards.empty()
      .append(metricCard("Deploy freq / week", fmt(s.deploy_frequency_weekly)))
      .append(metricCard("Lead time", fmt(s.lead_time_hours, "h")))
      .append(metricCard("Change-failure rate", fmt(s.change_failure_rate * 100, "%")))
      .append(metricCard("MTTR", fmt(s.mttr_hours, "h")));
  }

  function loadFleet() {
    var $cards = $("#dora-fleet");
    if (!$cards.length) { return; }
    $.ajax({ url: "/api/platform/dora", dataType: "json" })
      .done(function (d) {
        renderSummary($cards, d.fleet);
        var $a = $("#dora-apps").empty();
        var rows = d.applications || [];
        if (!rows.length) { $a.append($('<div class="text-xs text-slate-400">').text("No deployment events captured yet.")); return; }
        var $head = $('<div class="grid grid-cols-6 gap-2 text-xs font-semibold text-slate-500 border-b pb-1">')
          .append($("<div>").text("Application")).append($("<div>").text("Tier"))
          .append($("<div>").text("Freq/wk")).append($("<div>").text("Lead (h)"))
          .append($("<div>").text("CFR")).append($("<div>").text("MTTR (h)"));
        $a.append($head);
        rows.forEach(function (r) {
          var s = r.summary;
          var $name = $("<a>").attr("href", r.href).addClass("text-blue-600 hover:underline").text(r.name);
          $a.append($('<div class="grid grid-cols-6 gap-2 text-sm py-0.5 border-b border-slate-100">')
            .append($("<div>").append($name))
            .append($("<div>").append(tierBadge(s.tier)))
            .append($("<div>").text(fmt(s.deploy_frequency_weekly)))
            .append($("<div>").text(fmt(s.lead_time_hours)))
            .append($("<div>").text(fmt(s.change_failure_rate * 100, "%")))
            .append($("<div>").text(fmt(s.mttr_hours))));
        });
      })
      .fail(function () { $("#dora-apps").text("Could not load DORA metrics."); });
  }

  function loadApp() {
    var $panel = $("#dora-app");
    if (!$panel.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    $.ajax({ url: "/api/platform/applications/" + m[1] + "/dora", dataType: "json" })
      .done(function (d) {
        var s = d.summary;
        $panel.empty()
          .append($('<div class="text-xs text-slate-500 mb-1">').append($("<span>").text("Tier: ")).append(tierBadge(s.tier)))
          .append($('<div class="text-sm">').text(
            "Deploys/wk " + fmt(s.deploy_frequency_weekly) + " · lead " + fmt(s.lead_time_hours, "h") +
            " · CFR " + fmt(s.change_failure_rate * 100, "%") + " · MTTR " + fmt(s.mttr_hours, "h")))
          .append($('<div class="text-xs text-slate-400 mt-1">').text(s.deployments + " deploys · " + s.incidents + " incidents (last " + d.window_days + "d)"));
      })
      .fail(function () { $panel.text("Could not load DORA metrics."); });
  }

  $(function () { loadFleet(); loadApp(); });
})(jQuery);
