// Batch-change campaigns (M30): create a campaign across a filtered set of
// applications and track per-repository PR progress.
(function ($) {
  "use strict";

  function badge(status) {
    var cls = {
      pr_open: "bg-green-100 text-green-700",
      running: "bg-blue-100 text-blue-700",
      merged: "bg-green-100 text-green-700",
      failed: "bg-red-100 text-red-700",
    }[status] || "bg-slate-100 text-slate-600";
    return '<span class="text-xs rounded px-1.5 py-0.5 ' + cls + '">' +
      $("<span>").text((status || "").replace(/_/g, " ")).html() + "</span>";
  }

  function renderCampaign(c) {
    var $card = $('<div class="border border-slate-200 rounded p-3"></div>');
    $card.append('<div class="flex items-center justify-between gap-2">' +
      '<div class="font-medium text-sm">' + $("<span>").text(c.name).html() + "</div>" +
      badge(c.status) + "</div>");
    var $progress = $('<div class="mt-1 text-xs text-slate-500">Loading repositories…</div>');
    $card.append($progress);
    $.ajax({ url: "/api/platform/campaigns/" + c.id, dataType: "json", global: false }).done(function (d) {
      var ts = d.targets || [];
      var open = ts.filter(function (t) { return t.pr_url; }).length;
      var rows = ts.map(function (t) {
        return '<div class="flex items-center justify-between gap-2 py-0.5">' +
          '<span class="font-mono">' + $("<span>").text(t.branch_name).html() + "</span>" +
          '<span>' + badge(t.status) + (t.pr_url ? ' <a class="text-blue-600 hover:underline" target="_blank" href="' +
            $("<span>").text(t.pr_url).html() + '">PR</a>' : "") + "</span></div>";
      }).join("");
      $progress.html('<div class="mb-1">' + ts.length + " repositories · " + open + " PR(s)</div>" + rows);
    });
    return $card;
  }

  function load() {
    $.ajax({ url: "/api/platform/campaigns", dataType: "json", global: false }).done(function (d) {
      var $list = $("#camp-list").empty();
      var cs = d.campaigns || [];
      if (!cs.length) { $list.html('<div class="text-sm text-slate-400">No campaigns yet.</div>'); return; }
      cs.forEach(function (c) { $list.append(renderCampaign(c)); });
    });
  }

  function parseFilter(text) {
    text = (text || "").trim();
    if (!text) return null;
    var i = text.indexOf("=");
    if (i < 0) return null;
    var f = {};
    f[text.slice(0, i).trim()] = text.slice(i + 1).trim();
    return f;
  }

  function create() {
    var name = ($("#camp-name").val() || "").trim();
    var instruction = ($("#camp-instruction").val() || "").trim();
    if (!name || !instruction) { $("#camp-error").text("Name and instruction are required."); return; }
    var payload = { name: name, instruction: instruction };
    var filter = parseFilter($("#camp-filter").val());
    if (filter) payload.filter = filter;
    var $b = $("#camp-create").prop("disabled", true);
    $("#camp-error").text("");
    $.ajax({ url: "/api/platform/campaigns", method: "POST", contentType: "application/json", data: JSON.stringify(payload) })
      .done(function () {
        $("#camp-name").val(""); $("#camp-instruction").val(""); $("#camp-filter").val("");
        load();
      })
      .fail(function (xhr) {
        var err = xhr.responseJSON && xhr.responseJSON.error;
        $("#camp-error").text((err && err.message) || "Could not start the campaign");
      })
      .always(function () { $b.prop("disabled", false); });
  }

  $(function () {
    $("#camp-create").on("click", create);
    load();
  });
})(jQuery);
