// Auto-remediation (M46): the proposed-remediation queue (approve / dismiss),
// the rule list, and an on-demand "Evaluate now" sweep across the fleet.
(function ($) {
  "use strict";

  function loadRemediations() {
    $.ajax({ url: "/api/platform/remediations?status=proposed", dataType: "json" })
      .done(function (d) {
        var $r = $("#remediations").empty();
        var rows = d.remediations || [];
        if (!rows.length) { $r.append($('<div class="text-xs text-slate-400">').text("No pending remediations.")); return; }
        rows.forEach(function (r) {
          var $approve = $('<button class="text-xs text-green-700 hover:underline ml-2">').text("Approve")
            .on("click", function () { act("/api/platform/remediations/" + r.id + "/approve"); });
          var $dismiss = $('<button class="text-xs text-slate-500 hover:underline ml-2">').text("Dismiss")
            .on("click", function () { act("/api/platform/remediations/" + r.id + "/dismiss"); });
          $r.append($('<div class="py-1 border-b border-slate-100 flex justify-between text-sm">')
            .append($("<span>").text(r.finding_key))
            .append($("<span>").append($approve).append($dismiss)));
        });
      })
      .fail(function () { $("#remediations").text("Could not load remediations."); });
  }

  function act(url) {
    $.ajax({ url: url, method: "POST", contentType: "application/json", data: "{}" })
      .done(loadRemediations)
      .fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
  }

  function loadRules() {
    $.ajax({ url: "/api/platform/remediation/rules", dataType: "json" })
      .done(function (d) {
        var $r = $("#rules").empty();
        var rows = d.rules || [];
        if (!rows.length) { $r.append($('<div class="text-xs text-slate-400">').text("No rules yet.")); return; }
        rows.forEach(function (rule) {
          var $del = $('<button class="text-xs text-blue-600 hover:underline ml-2">').text("×")
            .on("click", function () { $.ajax({ url: "/api/platform/remediation/rules/" + rule.id, method: "DELETE" }).always(loadRules); });
          $r.append($('<div class="py-0.5 text-sm flex justify-between">')
            .append($("<span>").text(rule.name + " · " + rule.trigger_kind))
            .append($del));
        });
      })
      .fail(function () { $("#rules").text("Could not load rules."); });
  }

  $(function () {
    loadRemediations();
    loadRules();
    $("#rem-evaluate").on("click", function () {
      $.ajax({ url: "/api/platform/remediation/evaluate", method: "POST", contentType: "application/json", data: "{}" })
        .done(function (d) { alert((d.proposed || 0) + " remediation(s) proposed."); loadRemediations(); })
        .fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
    });
    $("#rule-add").on("click", function () {
      var params = {};
      try { params = JSON.parse($("#rule-params").val() || "{}"); } catch (e) { alert("Invalid params JSON"); return; }
      var payload = { name: $("#rule-name").val(), trigger_kind: $("#rule-trigger").val(), params: params, action: "agent_task", prompt: $("#rule-prompt").val() };
      $.ajax({ url: "/api/platform/remediation/rules", method: "POST", contentType: "application/json", data: JSON.stringify(payload) })
        .done(function () { $("#rule-name").val(""); $("#rule-params").val(""); $("#rule-prompt").val(""); loadRules(); })
        .fail(function (x) { alert("Failed: " + (x.responseText || x.status)); });
    });
  });
})(jQuery);
