// Job execution detail: poll status + logs until the run finishes.
(function ($) {
  "use strict";

  var id = JSON.parse($("#execution-id").text());
  var timer = null;

  function render(exec) {
    var rows = [
      ["Status", exec.status],
      ["Trigger", exec.trigger],
      ["Started", exec.started_at || "—"],
      ["Finished", exec.finished_at || "—"],
      ["Error", exec.error || "—"],
    ];
    var html = rows
      .map(function (r) {
        return '<div class="flex gap-2 text-sm"><span class="w-24 text-slate-500">' +
          r[0] + "</span><span>" + $("<div>").text(r[1]).html() + "</span></div>";
      })
      .join("");
    if (exec.summary) {
      html += '<div class="mt-2 text-xs text-slate-500">Summary: ' +
        $("<div>").text(JSON.stringify(exec.summary)).html() + "</div>";
    }
    $("#execution-summary").html(html);
    $("#execution-logs").text(exec.logs || "");
  }

  function poll() {
    // `global: false` keeps the recurring poll from flashing the loading mask.
    $.ajax({ url: "/api/jobs/executions/" + id, dataType: "json", global: false })
      .done(function (d) {
        render(d.execution);
        var done = ["succeeded", "failed", "cancelled"].indexOf(d.execution.status) >= 0;
        if (done && timer) { clearInterval(timer); timer = null; }
      });
  }

  $(function () {
    poll();
    timer = setInterval(poll, 1500);
  });
})(jQuery);
