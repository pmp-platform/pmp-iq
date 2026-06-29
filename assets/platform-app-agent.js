// Application "AI Agent" tab. Lists change tasks for the application, lets the
// user create a task (a session with an agentic AI that edits the repo and opens
// a PR), and shows each task's chat-style transcript with a follow-up box.
// Rendered into the tab panel by platform-app-detail.js via window.PIAgent.
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());
  var base = "/api/platform/applications/" + meta.id + "/agent-tasks";
  var listTimer = null;
  var taskTimer = null;
  var openTaskId = null;
  var ACTIVE = { running: 1 };

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "" : v).html();
  }

  function badge(status) {
    var cls = {
      pr_open: "bg-green-100 text-green-700",
      running: "bg-blue-100 text-blue-700",
      awaiting_input: "bg-amber-100 text-amber-700",
      failed: "bg-red-100 text-red-700",
    }[status] || "bg-slate-100 text-slate-600";
    var label = (status || "").replace(/_/g, " ");
    return '<span class="text-xs rounded px-1.5 py-0.5 ' + cls + '">' + esc(label) + "</span>";
  }

  // Render agent/user message content as Markdown (sanitised) or plain text.
  function messageHtml(text) {
    var $body = $('<div class="md-body text-sm"></div>');
    if (typeof marked !== "undefined") {
      var $html = $("<div>").html(marked.parse(text || ""));
      $html.find("script, style, iframe").remove();
      $body.append($html);
    } else {
      $body.text(text || "");
    }
    return $body;
  }

  function renderTaskList(tasks) {
    var $list = $("#agent-task-list").empty();
    if (!tasks.length) {
      $list.html('<div class="text-sm text-slate-400">No tasks yet. Create one above.</div>');
      return;
    }
    tasks.forEach(function (t) {
      var $card = $('<div class="border border-slate-200 rounded p-3"></div>');
      var $head = $('<div class="flex items-center justify-between gap-2"></div>');
      $head.append('<button type="button" class="text-sm font-medium text-blue-700 hover:underline text-left"></button>');
      $head.find("button").text(t.title).on("click", function () { openTask(t.id); });
      $head.append('<div class="flex items-center gap-2">' + badge(t.status) +
        (t.pr_url ? '<a class="text-xs text-blue-600 hover:underline" target="_blank" href="' +
          esc(t.pr_url) + '">View PR</a>' : "") + "</div>");
      $card.append($head);
      $card.append('<div id="agent-task-' + t.id + '" class="mt-2 hidden"></div>');
      $list.append($card);
    });
    if (openTaskId) openTask(openTaskId, true);
  }

  function loadTasks() {
    $.ajax({ url: base, dataType: "json", global: false }).done(function (d) {
      var tasks = d.tasks || [];
      renderTaskList(tasks);
      var active = tasks.some(function (t) { return ACTIVE[t.status]; });
      if (active && !listTimer) {
        listTimer = setInterval(loadTasks, 2000);
      } else if (!active && listTimer) {
        clearInterval(listTimer);
        listTimer = null;
      }
    });
  }

  function renderTargets($into, targets) {
    if (!targets || targets.length <= 1) return; // single-repo: nothing extra to show
    var $box = $('<div class="mb-2 text-xs border border-slate-200 rounded p-2"></div>');
    $box.append('<div class="font-semibold text-slate-500 mb-1">Repositories (' + targets.length + ')</div>');
    targets.forEach(function (t) {
      var $row = $('<div class="flex items-center justify-between gap-2 py-0.5"></div>');
      $row.append($('<span class="font-mono"></span>').text(t.branch_name));
      var right = badge(t.status) + (t.pr_url ? ' <a class="text-blue-600 hover:underline" target="_blank" href="' +
        esc(t.pr_url) + '">PR</a>' : "");
      $row.append($('<span></span>').html(right));
      $box.append($row);
    });
    $into.append($box);
  }

  function renderTranscript($into, data) {
    var task = data.task || {};
    var messages = data.messages || [];
    $into.empty();
    renderTargets($into, data.targets);
    messages.forEach(function (m) {
      var mine = m.role === "user";
      var $row = $('<div class="mb-2"></div>');
      $row.append('<div class="text-xs font-semibold uppercase tracking-wide ' +
        (mine ? "text-slate-500" : "text-blue-600") + '">' + esc(m.role) + "</div>");
      $row.append(messageHtml(m.content));
      if (m.execution_id) {
        $row.append('<a class="text-xs text-blue-600 hover:underline" href="/jobs/executions/' +
          m.execution_id + '">view full agent run</a>');
      }
      $into.append($row);
    });
    if (task.status === "running") {
      $into.append('<div class="text-xs text-slate-400">Working…</div>');
    }
    // Follow-up composer.
    var $form = $('<div class="mt-2 border-t border-slate-100 pt-2 space-y-2"></div>');
    var $ta = $('<textarea rows="2" class="w-full border border-slate-300 rounded px-2 py-1 text-sm" ' +
      'placeholder="Send a follow-up instruction…"></textarea>');
    var $btn = $('<button type="button" class="bg-blue-100 text-blue-700 rounded px-2.5 py-1 text-xs font-medium hover:bg-blue-200 disabled:opacity-50">Send</button>');
    $btn.on("click", function () { sendMessage(task.id, $ta, $btn); });
    $form.append($ta).append('<div class="flex justify-end">').find("div").append($btn);
    $into.append($form);
  }

  function openTask(taskId, keepOpen) {
    openTaskId = taskId;
    $("[id^=agent-task-]").addClass("hidden");
    var $panel = $("#agent-task-" + taskId).removeClass("hidden");
    if (!keepOpen) $panel.html('<div class="text-xs text-slate-400">Loading…</div>');
    $.ajax({ url: base + "/" + taskId, dataType: "json", global: false }).done(function (d) {
      renderTranscript($panel, d);
      if (taskTimer) { clearInterval(taskTimer); taskTimer = null; }
      if (d.task && d.task.status === "running") {
        taskTimer = setInterval(function () { openTask(taskId, true); }, 2000);
      }
    });
  }

  function sendMessage(taskId, $ta, $btn) {
    var message = ($ta.val() || "").trim();
    if (!message) return;
    $btn.prop("disabled", true);
    $.ajax({
      url: base + "/" + taskId + "/messages", method: "POST",
      contentType: "application/json", data: JSON.stringify({ message: message }),
    })
      .done(function () { $ta.val(""); $btn.prop("disabled", false); openTask(taskId, true); loadTasks(); })
      .fail(function (xhr) {
        $btn.prop("disabled", false);
        var err = xhr.responseJSON && xhr.responseJSON.error;
        alert((err && err.message) || "Could not send the message");
      });
  }

  function createTask() {
    var title = ($("#agent-new-title").val() || "").trim();
    var message = ($("#agent-new-message").val() || "").trim();
    if (!title || !message) return;
    var $btn = $("#agent-create").prop("disabled", true);
    $.ajax({
      url: base, method: "POST", contentType: "application/json",
      data: JSON.stringify({ title: title, message: message }),
    })
      .done(function (d) {
        $("#agent-new-title").val("");
        $("#agent-new-message").val("");
        $btn.prop("disabled", false);
        if (d.task) openTaskId = d.task.id;
        loadTasks();
      })
      .fail(function (xhr) {
        $btn.prop("disabled", false);
        var err = xhr.responseJSON && xhr.responseJSON.error;
        $("#agent-create-error").text((err && err.message) || "Could not create the task");
      });
  }

  function renderInto($panel) {
    $panel.html(
      '<div class="space-y-4">' +
        '<div class="bg-white rounded-lg shadow border border-slate-200 p-4 space-y-2">' +
          '<div class="text-sm font-semibold">New task</div>' +
          '<input id="agent-new-title" class="w-full border border-slate-300 rounded px-2 py-1 text-sm" placeholder="Task title (e.g. Add a /health endpoint)" />' +
          '<textarea id="agent-new-message" rows="3" class="w-full border border-slate-300 rounded px-2 py-1 text-sm" placeholder="Describe the change you want the agent to make…"></textarea>' +
          '<div class="flex items-center justify-between gap-2">' +
            '<span id="agent-create-error" class="text-xs text-red-600"></span>' +
            '<button id="agent-create" type="button" class="bg-blue-100 text-blue-700 rounded px-2.5 py-1 text-xs font-medium hover:bg-blue-200 disabled:opacity-50">Create task</button>' +
          "</div>" +
        "</div>" +
        '<div id="agent-task-list" class="space-y-2"></div>' +
      "</div>"
    );
    $("#agent-create").on("click", createTask);
    loadTasks();
  }

  window.PIAgent = { render: renderInto };
})(jQuery);
