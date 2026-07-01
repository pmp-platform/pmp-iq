// Timeline & audit views (M36): renders the audit log + global change feed on
// the Audit page, and the per-application change timeline on the app detail.
(function ($) {
  "use strict";

  function changeBadge(kind) {
    var color = kind === "created" ? "text-green-600" : kind === "removed" ? "text-red-600" : "text-amber-600";
    return $('<span class="font-medium">').addClass(color).text(kind);
  }

  function renderChanges($target, changes) {
    $target.empty();
    if (!changes || !changes.length) { $target.append($('<div class="text-xs text-slate-400">').text("No recorded changes.")); return; }
    changes.forEach(function (c) {
      $target.append(
        $('<div class="py-0.5 border-b border-slate-100 flex gap-2 items-baseline">')
          .append(changeBadge(c.change))
          .append($('<span>').text(c.entity_type + " · " + c.entity_key))
          .append($('<span class="text-xs text-slate-400 ml-auto">').text((c.occurred_at || "").replace("T", " ").slice(0, 19)))
      );
    });
  }

  function loadAudit() {
    var $rows = $("#audit-rows");
    if (!$rows.length) { return; }
    $.ajax({ url: "/api/audit", dataType: "json" })
      .done(function (d) {
        $rows.empty();
        (d.events || []).forEach(function (e) {
          $rows.append($("<tr class='border-b border-slate-100'>")
            .append($('<td class="p-2.5 text-slate-500">').text((e.occurred_at || "").replace("T", " ").slice(0, 19)))
            .append($("<td>").text(e.actor))
            .append($("<td>").text(e.action))
            .append($("<td class='text-slate-500'>").text(e.target || "")));
        });
        if (!(d.events || []).length) { $rows.append("<tr><td colspan='4' class='p-2.5 text-slate-400'>No audit events.</td></tr>"); }
      })
      .fail(function () { $rows.html("<tr><td colspan='4' class='p-2.5 text-red-600'>Could not load audit log.</td></tr>"); });
  }

  function loadGlobalTimeline() {
    var $t = $("#global-timeline");
    if (!$t.length) { return; }
    $.ajax({ url: "/api/platform/timeline", dataType: "json" })
      .done(function (d) { renderChanges($t, d.changes); })
      .fail(function () { $t.text("Could not load timeline."); });
  }

  function loadAppTimeline() {
    var $t = $("#app-timeline");
    if (!$t.length) { return; }
    var m = location.pathname.match(/\/platform\/applications\/([0-9a-f-]+)/i);
    if (!m) { return; }
    $.ajax({ url: "/api/platform/applications/" + m[1] + "/timeline", dataType: "json" })
      .done(function (d) { renderChanges($t, d.changes); })
      .fail(function () { $t.text("Could not load timeline."); });
  }

  $(function () {
    loadAudit();
    loadGlobalTimeline();
    loadAppTimeline();
  });
})(jQuery);
