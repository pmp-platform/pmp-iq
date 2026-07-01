// Application Q&A. A single "Ask a Question" button (added to the page header by
// the detail view) opens this modal with two tabs: "Ask a question" (the form)
// and "Previous questions" (history of questions asked / being processed).
// Submitting queues an llm-repository-request execution for the app's repository.
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());
  if (meta.entity !== "applications") {
    window.PIAsk = { open: function () {} };
    return;
  }

  var base = "/api/platform/applications/" + meta.id + "/ask";
  var pollTimer = null;
  var built = false;
  var DONE = { succeeded: 1, failed: 1, cancelled: 1 };

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "" : v).html();
  }

  function badge(status) {
    var cls = {
      succeeded: "bg-green-100 text-green-700",
      failed: "bg-red-100 text-red-700",
      skipped: "bg-amber-100 text-amber-700",
    }[status] || "bg-slate-100 text-slate-600";
    return '<span class="text-xs rounded px-1.5 py-0.5 ' + cls + '">' + esc(status) + "</span>";
  }

  function showTab(which) {
    $("#ask-tab-form").toggleClass("hidden", which !== "form");
    $("#ask-tab-history").toggleClass("hidden", which !== "history");
    $("#ask-tabbtn-form").toggleClass("border-slate-900 text-slate-900 font-medium", which === "form")
      .toggleClass("border-transparent text-slate-500", which !== "form");
    $("#ask-tabbtn-history").toggleClass("border-slate-900 text-slate-900 font-medium", which === "history")
      .toggleClass("border-transparent text-slate-500", which !== "history");
    if (which === "history") loadHistory();
  }

  function buildModal() {
    if (built) return;
    built = true;
    var tabBtn = 'px-3 py-2 border-b-2 -mb-px text-sm border-transparent text-slate-500';
    $("body").append(
      '<div id="ask-modal" class="modal hidden fixed inset-0 bg-black/40 z-50 items-center justify-center p-4">' +
        '<div class="bg-white rounded-lg shadow-lg w-[90vw] max-w-2xl max-h-[90vh] flex flex-col">' +
          '<div class="flex items-center justify-between p-4 border-b border-slate-200">' +
            '<h3 class="text-lg font-semibold">Ask the LLM about this application</h3>' +
            '<button type="button" data-modal-close class="text-2xl leading-none text-slate-400 hover:text-slate-700">&times;</button>' +
          "</div>" +
          '<div class="flex gap-1 px-4 pt-2 border-b border-slate-200">' +
            '<button type="button" id="ask-tabbtn-form" class="' + tabBtn + '">Ask a question</button>' +
            '<button type="button" id="ask-tabbtn-history" class="' + tabBtn + '">Previous questions</button>' +
          "</div>" +
          '<div class="p-4 overflow-auto">' +
            '<div id="ask-tab-form" class="space-y-2">' +
              '<textarea id="ask-input" rows="3" class="w-full border border-slate-300 rounded px-2 py-1 text-sm" ' +
                'placeholder="e.g. How does authentication work in this repository?"></textarea>' +
              '<div class="flex items-center justify-between gap-2">' +
                '<span id="ask-status" class="text-xs text-slate-500"></span>' +
                '<button id="ask-submit" type="button" class="btn btn-primary btn-sm">Ask</button>' +
              "</div>" +
            "</div>" +
            '<div id="ask-tab-history" class="hidden space-y-2"></div>' +
          "</div>" +
        "</div>" +
      "</div>"
    );
    $("#ask-tabbtn-form").on("click", function () { showTab("form"); });
    $("#ask-tabbtn-history").on("click", function () { showTab("history"); });
    $("#ask-submit").on("click", ask);
    $("#ask-input").on("keydown", function (e) {
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") ask();
    });
  }

  function renderHistory(items) {
    var $list = $("#ask-tab-history").empty();
    if (!items.length) {
      $list.html('<div class="text-sm text-slate-400">No questions yet.</div>');
      return;
    }
    items.forEach(function (q) {
      var $card = $('<div class="border border-slate-200 rounded p-3"></div>');
      // Question block.
      $card.append('<div class="flex items-center justify-between gap-2 mb-1">' +
        '<div class="text-xs font-semibold uppercase tracking-wide text-slate-500">Question</div>' +
        badge(q.status) + "</div>");
      $card.append('<div class="text-sm text-slate-800 mb-2">' + esc(q.question) + "</div>");
      // Answer block (rendered as Markdown), separated from the question.
      if (q.answer) {
        $card.append('<div class="text-xs font-semibold uppercase tracking-wide text-slate-500 border-t border-slate-100 pt-2 mb-1">Answer</div>');
        $card.append(answerHtml(q.answer));
      } else if (!DONE[q.status]) {
        $card.append('<div class="text-xs text-slate-400">Thinking…</div>');
      }
      $card.append('<a class="text-xs text-blue-600 hover:underline" href="/jobs/executions/' +
        q.execution_id + '">view full LLM run</a>');
      $list.append($card);
    });
  }

  // Render an answer as Markdown (sanitised), falling back to plain text.
  function answerHtml(answer) {
    var $ans = $('<div class="md-body text-sm mb-2"></div>');
    if (typeof marked !== "undefined") {
      var $html = $("<div>").html(marked.parse(answer));
      $html.find("script, style, iframe").remove();
      $ans.append($html);
    } else {
      $ans.text(answer);
    }
    return $ans;
  }

  function loadHistory() {
    $.ajax({ url: base, dataType: "json", global: false }).done(function (d) {
      var items = d.questions || [];
      renderHistory(items);
      var pending = items.some(function (q) { return !DONE[q.status]; });
      if (pending && !pollTimer) {
        pollTimer = setInterval(loadHistory, 1500);
      } else if (!pending && pollTimer) {
        clearInterval(pollTimer);
        pollTimer = null;
      }
    });
  }

  function ask() {
    var question = ($("#ask-input").val() || "").trim();
    if (!question) return;
    $("#ask-submit").prop("disabled", true);
    $("#ask-status").text("Queuing…");
    $.ajax({
      url: base, method: "POST", contentType: "application/json",
      data: JSON.stringify({ question: question }),
    })
      .done(function () {
        $("#ask-input").val("");
        $("#ask-status").text("");
        $("#ask-submit").prop("disabled", false);
        showTab("history");
      })
      .fail(function (xhr) {
        var err = xhr.responseJSON && xhr.responseJSON.error;
        $("#ask-status").text("Error: " + ((err && err.message) || "could not queue the question"));
        $("#ask-submit").prop("disabled", false);
      });
  }

  window.PIAsk = {
    open: function () {
      buildModal();
      showTab("form");
      PI.openModal("#ask-modal");
    },
  };
})(jQuery);
