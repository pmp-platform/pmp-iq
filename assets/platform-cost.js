// LLM cost panel (M39): fetches /api/platform/cost and renders period spend,
// top spenders by job type / application, and budget status.
(function ($) {
  "use strict";

  function usd(n) { return "$" + (Number(n) || 0).toFixed(2); }

  function tile(label, value) {
    return $('<div class="bg-white rounded-lg shadow border border-slate-200 p-3">')
      .append($('<div class="text-xs text-slate-500">').text(label))
      .append($('<div class="text-lg font-semibold">').text(value));
  }

  function rows(target, items, labelKey) {
    var $t = $(target).empty();
    if (!items || !items.length) { $t.append($('<div class="text-xs text-slate-400">').text("No spend yet.")); return; }
    items.forEach(function (r) {
      $t.append(
        $('<div class="flex justify-between text-sm py-0.5 border-b border-slate-100">')
          .append($('<span class="truncate mr-2">').text(r[labelKey] || r.key))
          .append($('<span class="font-mono">').text(usd(r.cost_usd)))
      );
    });
  }

  function budgets(items) {
    var $t = $("#cost-budgets").empty();
    if (!items || !items.length) {
      $t.append($('<div class="text-xs text-slate-400 mb-2">').text("No budgets configured."));
    }
    (items || []).forEach(function (b) {
      var cls = b.over ? "text-red-600 font-semibold" : "text-slate-700";
      var $del = $('<button class="text-xs text-blue-600 hover:underline ml-2">').text("delete")
        .on("click", function () { deleteBudget(b.id); });
      $t.append(
        $('<div class="text-sm py-0.5 border-b border-slate-100 flex justify-between items-center">')
          .append($('<span>').addClass(cls).text(
            b.scope + (b.scope_id ? " · " + b.scope_id.slice(0, 8) : "") + " · " + b.period +
            ": " + usd(b.spent_usd) + " / " + usd(b.limit_usd) +
            (b.hard_stop ? " (hard-stop)" : "") + (b.over ? " ⛔" : "")
          ))
          .append($del)
      );
    });
    $t.append(budgetForm());
  }

  // A compact create-budget form (global/profile/job/application × daily/monthly).
  function budgetForm() {
    var $scope = $('<select class="border rounded text-xs p-1">');
    ["global", "profile", "job", "application"].forEach(function (s) { $scope.append($('<option>').val(s).text(s)); });
    var $period = $('<select class="border rounded text-xs p-1">');
    ["monthly", "daily"].forEach(function (p) { $period.append($('<option>').val(p).text(p)); });
    var $scopeId = $('<input class="border rounded text-xs p-1 w-28" placeholder="scope id (uuid)">');
    var $limit = $('<input type="number" min="0" step="0.01" class="border rounded text-xs p-1 w-20" placeholder="USD">');
    var $hard = $('<input type="checkbox">');
    var $add = $('<button class="btn btn-primary btn-sm">').text("Add");
    $add.on("click", function () {
      var payload = { scope: $scope.val(), period: $period.val(), limit_usd: parseFloat($limit.val()), hard_stop: $hard.is(":checked") };
      if ($scopeId.val()) { payload.scope_id = $scopeId.val(); }
      $.ajax({ url: "/api/cost/budgets", method: "POST", contentType: "application/json", data: JSON.stringify(payload) })
        .done(load)
        .fail(function (x) { alert("Could not create budget: " + (x.responseText || x.status)); });
    });
    return $('<div class="flex flex-wrap items-center gap-1 mt-2">')
      .append($scope).append($period).append($scopeId).append($limit)
      .append($('<label class="text-xs flex items-center gap-1">').append($hard).append("hard"))
      .append($add);
  }

  function deleteBudget(id) {
    $.ajax({ url: "/api/cost/budgets/" + id, method: "DELETE" }).always(load);
  }

  function load() {
    $.ajax({ url: "/api/platform/cost", dataType: "json" })
      .done(function (d) {
        var $tot = $("#cost-totals").empty();
        $tot.append(tile("Spend this month", usd(d.spend_this_month)));
        $tot.append(tile("Spend today", usd(d.spend_today)));
        $tot.append(tile("Projected month-end", usd(d.projected_month_end)));
        $tot.append(tile("Budgets", (d.budgets || []).length));
        rows("#cost-by-job-type", d.by_job_type, "key");
        rows("#cost-by-application", d.by_application, "key");
        budgets(d.budgets);
      })
      .fail(function () { $("#cost-totals").text("Could not load cost data."); });
  }

  $(load);
})(jQuery);
