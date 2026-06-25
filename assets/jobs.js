// Jobs page: job CRUD, manual runs, and live execution table.
(function ($) {
  "use strict";

  var REVIEW_TYPE = "review-repositories";
  var typeDescriptions = {};

  // Populate the job-type select and its description from the registry.
  function loadJobTypes() {
    return $.getJSON("/api/jobs/types").done(function (d) {
      var $sel = $("#job-type").empty();
      (d.types || []).forEach(function (t) {
        typeDescriptions[t.id] = t.description;
        $sel.append($("<option>").val(t.id).text(t.id));
      });
      updateJobTypeUI();
    });
  }

  // Populate the AI-profile select (used by the review-repositories job type).
  function loadAiProfiles() {
    return $.getJSON("/api/settings/ai-profiles").done(function (d) {
      var $sel = $("#ai-profile");
      (d.profiles || []).forEach(function (p) {
        $sel.append($("<option>").val(p.id).text(p.name + (p.enabled ? "" : " (disabled)")));
      });
    });
  }

  // Show the description and toggle AI-profile vs raw-config inputs by type.
  function updateJobTypeUI() {
    var t = $("#job-type").val();
    $("#job-type-desc").text(typeDescriptions[t] || "");
    var isReview = t === REVIEW_TYPE;
    $("#ai-profile-field").toggleClass("hidden", !isReview);
    $("#job-config").toggleClass("hidden", isReview);
  }

  function statusBadge(status) {
    var colors = {
      queued: "bg-slate-200 text-slate-700",
      running: "bg-blue-100 text-blue-700",
      paused: "bg-amber-100 text-amber-700",
      succeeded: "bg-green-100 text-green-700",
      failed: "bg-red-100 text-red-700",
      cancelled: "bg-amber-100 text-amber-700",
    };
    var cls = colors[status] || "bg-slate-200";
    return '<span class="px-2 py-0.5 rounded text-xs ' + cls + '">' + status + "</span>";
  }

  function renderJobs(jobs) {
    var $body = $("#jobs-table tbody").empty();
    if (!jobs.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="5">No jobs yet.</td></tr>');
      return;
    }
    jobs.forEach(function (j) {
      var trig = j.trigger_type + (j.cron_expr ? " (" + j.cron_expr + ")" : "");
      var $row = $(
        '<tr class="border-b">' +
          '<td class="p-3">' + $("<div>").text(j.name).html() + "</td>" +
          "<td>" + j.job_type + "</td>" +
          "<td>" + $("<div>").text(trig).html() + "</td>" +
          "<td>" + (j.enabled ? "yes" : "no") + "</td>" +
          '<td class="text-right pr-3 whitespace-nowrap">' +
            window.PI.actionButton("Run now", { "data-act": "run" }, "success") +
            window.PI.actionButton("Delete", { "data-act": "del" }, "danger") +
          "</td>" +
        "</tr>"
      );
      $row.find("button").data("id", j.id);
      $body.append($row);
    });
  }

  function renderExecutions(execs) {
    var $body = $("#executions-table tbody").empty();
    if (!execs.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="5">No executions yet.</td></tr>');
      return;
    }
    execs.forEach(function (e) {
      var action = "";
      if (e.status === "running" || e.status === "queued") {
        action = window.PI.actionButton("Pause", { "data-exec-act": "pause" }, "warn");
      } else if (e.status === "paused") {
        action = window.PI.actionButton("Resume", { "data-exec-act": "resume" }, "success");
      }
      var $row = $(
        '<tr class="border-b">' +
          '<td class="p-3">' + statusBadge(e.status) +
            (e.resume_at ? ' <span class="text-xs text-slate-400">resumes ' + e.resume_at + "</span>" : "") + "</td>" +
          "<td>" + e.trigger + "</td>" +
          "<td>" + (e.started_at || "—") + "</td>" +
          "<td>" + (e.finished_at || "—") + "</td>" +
          '<td class="text-right pr-3 whitespace-nowrap">' + action +
            window.PI.linkButton("View", "/jobs/executions/" + e.id) + "</td>" +
        "</tr>"
      );
      $row.find("button").data("id", e.id);
      $body.append($row);
    });
  }

  function loadJobs() {
    $.getJSON("/api/jobs").done(function (d) { renderJobs(d.jobs); });
  }
  function loadExecutions() {
    // `global: false` keeps the recurring poll from flashing the loading mask.
    $.ajax({ url: "/api/jobs/executions", dataType: "json", global: false })
      .done(function (d) { renderExecutions(d.executions); });
  }
  function flash(msg, ok) {
    $("#job-msg").text(msg).css("color", ok ? "#15803d" : "#b91c1c");
  }

  $(function () {
    loadJobs();
    loadExecutions();
    loadJobTypes();
    loadAiProfiles();
    setInterval(loadExecutions, 3000);

    $("#job-type").on("change", updateJobTypeUI);

    $("#job-form").on("submit", function (e) {
      e.preventDefault();
      var data = {};
      $.each($(this).serializeArray(), function (_, f) { if (f.value) data[f.name] = f.value; });
      if (data.job_type === REVIEW_TYPE) {
        var profileId = $("#ai-profile").val();
        data.config = profileId ? { ai_profile_id: profileId } : {};
      } else if (data.config) {
        try { data.config = JSON.parse(data.config); }
        catch (err) { flash("Invalid config JSON", false); return; }
      }
      $.ajax({
        url: "/api/jobs",
        method: "POST",
        contentType: "application/json",
        data: JSON.stringify(data),
      }).done(function () {
        flash("", true);
        loadJobs();
        $("#job-form")[0].reset();
        updateJobTypeUI();
        window.PI.closeModal("#job-modal");
      }).fail(function (x) { flash("Error: " + x.responseText, false); });
    });

    $("#executions-table").on("click", "button", function () {
      var id = $(this).data("id");
      var act = $(this).data("exec-act");
      if (act === "pause") {
        $.ajax({ url: "/api/jobs/executions/" + id + "/pause", method: "POST" })
          .done(function () { window.PI.toast("Pause requested", true); loadExecutions(); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      } else if (act === "resume") {
        $.ajax({ url: "/api/jobs/executions/" + id + "/resume", method: "POST" })
          .done(function () { window.PI.toast("Resumed", true); loadExecutions(); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      }
    });

    $("#jobs-table").on("click", "button", function () {
      var id = $(this).data("id");
      var act = $(this).data("act");
      if (act === "del") {
        window.PI.confirm("Delete this job?", function () {
          $.ajax({ url: "/api/jobs/" + id, method: "DELETE" }).done(loadJobs);
        });
      } else if (act === "run") {
        $.ajax({ url: "/api/jobs/" + id + "/run", method: "POST" })
          .done(function () { window.PI.toast("Started", true); loadExecutions(); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      }
    });
  });
})(jQuery);
