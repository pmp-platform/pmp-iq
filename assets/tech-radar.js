// Tech radar + fleet currency (M45). The radar is grouped by ring; the currency
// report ranks the least-current applications.
(function ($) {
  "use strict";

  var RINGS = ["adopt", "trial", "assess", "hold"];

  function loadRadar() {
    $.ajax({ url: "/api/platform/tech-radar", dataType: "json" })
      .done(function (d) {
        var $r = $("#radar").empty();
        var byRing = {};
        (d.radar || []).forEach(function (e) { (byRing[e.ring] = byRing[e.ring] || []).push(e); });
        var $grid = $('<div class="grid grid-cols-2 md:grid-cols-4 gap-3">');
        RINGS.forEach(function (ring) {
          var $col = $('<div>').append($('<div class="text-xs font-semibold capitalize mb-1">').text(ring));
          (byRing[ring] || []).forEach(function (e) {
            var $del = $('<button class="text-xs text-blue-600 hover:underline ml-1">').text("×")
              .on("click", function () { $.ajax({ url: "/api/platform/tech-radar/" + e.id, method: "DELETE" }).always(loadRadar); });
            $col.append($('<div class="text-xs py-0.5">').text(e.quadrant + " · " + e.name).append($del));
          });
          $grid.append($col);
        });
        $r.append($grid);
        if (!(d.radar || []).length) { $r.append($('<div class="text-xs text-slate-400">').text("No radar entries yet.")); }
      })
      .fail(function () { $("#radar").text("Could not load the radar."); });
  }

  function loadCurrency() {
    $.ajax({ url: "/api/platform/currency", dataType: "json" })
      .done(function (d) {
        var $c = $("#fleet-currency").empty();
        var rows = (d.currency || []).filter(function (r) { return r.score < 1 || r.eol_count > 0; });
        if (!rows.length) { $c.append($('<div class="text-xs text-slate-400">').text("Everything is current.")); return; }
        rows.forEach(function (r) {
          var $name = r.href ? $("<a>").attr("href", r.href).addClass("text-blue-600 hover:underline").text(r.name) : $("<span>").text(r.name);
          $c.append($('<div class="py-0.5 border-b border-slate-100 flex justify-between text-sm">')
            .append($name)
            .append($("<span>").text(Math.round((r.score || 0) * 100) + "% current" + (r.eol_count ? " · " + r.eol_count + " EOL" : ""))));
        });
      })
      .fail(function () { $("#fleet-currency").text("Could not load currency."); });
  }

  // Per-application dependency currency (app detail panel).
  function loadAppCurrency() {
    var $panel = $("#dep-currency");
    if (!$panel.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    $.ajax({ url: "/api/platform/applications/" + m[1] + "/currency", dataType: "json" })
      .done(function (d) {
        $panel.empty();
        $panel.append($('<div class="text-xs text-slate-500 mb-1">').text(Math.round((d.score || 0) * 100) + "% current"));
        var outdated = (d.dependencies || []).filter(function (x) { return x.major_behind > 0 || x.eol_status === "eol" || x.eol_status === "eol_soon"; });
        if (!outdated.length) { $panel.append($('<div class="text-xs text-slate-400">').text("All dependencies are current.")); return; }
        outdated.forEach(function (x) {
          var note = x.major_behind > 0 ? (x.major_behind + " major behind (" + x.version + "→" + (x.latest || "?") + ")") : x.eol_status;
          $panel.append($('<div class="py-0.5 text-sm">')
            .append($('<span class="font-mono">').text(x.ecosystem ? x.ecosystem + ":" + x.name : x.name))
            .append($('<span class="text-red-600 ml-2 text-xs">').text(note)));
        });
      })
      .fail(function () { $panel.text("Could not load currency."); });
  }

  $(function () {
    loadRadar();
    loadCurrency();
    loadAppCurrency();
    $("#radar-add").on("click", function () {
      var payload = { quadrant: $("#radar-quadrant").val(), name: $("#radar-name").val(), ring: $("#radar-ring").val(), note: $("#radar-note").val() || undefined };
      $.ajax({ url: "/api/platform/tech-radar", method: "POST", contentType: "application/json", data: JSON.stringify(payload) })
        .done(function () { $("#radar-name").val(""); $("#radar-note").val(""); loadRadar(); })
        .fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
    });
  });
})(jQuery);
