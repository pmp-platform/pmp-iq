// Job execution detail: poll status + output until the run finishes. A
// "More Details" button opens a modal with the full execution record (summary,
// metadata, params, state) as prettified JSON.
(function ($) {
  "use strict";

  var id = JSON.parse($("#execution-id").text());
  var timer = null;
  var last = null;

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "—" : v).html();
  }

  function row(label, value) {
    return '<div class="flex gap-2 text-sm"><span class="w-24 text-slate-500">' +
      label + "</span><span>" + esc(value) + "</span></div>";
  }

  function render(exec) {
    last = exec;
    var html =
      row("Status", exec.status) +
      row("Trigger", exec.trigger) +
      row("Started", exec.started_at || "—") +
      row("Finished", exec.finished_at || "—") +
      row("Error", exec.error || "—");
    $("#execution-summary").html(html);
    $("#execution-logs").text(exec.logs || "");
  }

  // A labelled block of prettified JSON (omitted when empty).
  function jsonBlock(label, value) {
    if (value === null || value === undefined) return "";
    if (typeof value === "object" && !Array.isArray(value) && !Object.keys(value).length) return "";
    return '<div><div class="text-xs font-semibold text-slate-500 mb-1">' + label + "</div>" +
      '<pre class="bg-slate-50 border border-slate-200 rounded p-2 text-xs overflow-auto">' +
      esc(JSON.stringify(value, null, 2)) + "</pre></div>";
  }

  function openDetails() {
    if (!last) return;
    var html =
      row("Id", last.id) +
      row("Job", last.job_id) +
      row("Status", last.status) +
      row("Trigger", last.trigger) +
      row("Started", last.started_at || "—") +
      row("Finished", last.finished_at || "—") +
      row("Resume at", last.resume_at || "—") +
      row("Pause requested", last.pause_requested) +
      (last.error ? jsonBlock("Error", last.error) : "") +
      jsonBlock("Summary", last.summary) +
      jsonBlock("Metadata", last.metadata) +
      jsonBlock("Params", last.params) +
      jsonBlock("State", last.state);
    $("#exec-modal-body").html(html);
    PI.openModal("#exec-modal");
  }

  function poll() {
    // `global: false` keeps the recurring poll from flashing the loading mask.
    $.ajax({ url: "/api/jobs/executions/" + id, dataType: "json", global: false })
      .done(function (d) {
        render(d.execution);
        var done = ["succeeded", "failed", "cancelled", "skipped"].indexOf(d.execution.status) >= 0;
        if (done && timer) { clearInterval(timer); timer = null; }
      });
  }

  $(function () {
    $("#exec-more").on("click", openDetails);
    poll();
    timer = setInterval(poll, 1500);
  });
})(jQuery);
