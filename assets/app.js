// Front-end behaviour shared across pages: sidebar nav, tabs, and modals.
(function ($) {
  "use strict";

  var ACTIVE = "bg-slate-800 text-white";

  // Highlight the active sidebar item from the <main data-active> hint.
  function highlightNav() {
    var active = $("main").data("active");
    if (!active) return;
    $('#sidebar a[data-nav="' + active + '"]').addClass(ACTIVE);
  }

  // Tabs: a [data-tabs] container with [data-tab] buttons and [data-panel]s.
  function initTabs() {
    $("[data-tabs]").each(function () {
      var $group = $(this);
      function activate(key) {
        $group.find("[data-tab]")
          .removeClass("border-slate-900 text-slate-900 font-medium")
          .addClass("border-transparent text-slate-500");
        $group.find('[data-tab="' + key + '"]')
          .addClass("border-slate-900 text-slate-900 font-medium")
          .removeClass("border-transparent text-slate-500");
        $group.find("[data-panel]").addClass("hidden");
        $group.find('[data-panel="' + key + '"]').removeClass("hidden");
      }
      $group.on("click", "[data-tab]", function () { activate($(this).data("tab")); });
      activate($group.find("[data-tab]").first().data("tab"));
    });
  }

  function openModal(sel) { $(sel).removeClass("hidden").addClass("flex"); }
  function closeModal(sel) { $(sel).addClass("hidden").removeClass("flex"); }

  // Generic modal wiring: open via [data-modal-open=id], close via
  // [data-modal-close] or a backdrop click.
  function initModals() {
    $(document).on("click", "[data-modal-open]", function () {
      openModal("#" + $(this).data("modal-open"));
    });
    $(document).on("click", "[data-modal-close]", function () {
      closeModal($(this).closest(".modal"));
    });
    $(document).on("click", ".modal", function (e) {
      if (e.target === this) closeModal(this);
    });
    $(document).on("keydown", function (e) {
      if (e.key === "Escape") closeModal(".modal");
    });
  }

  // Transient toast for actions taken outside a modal (table buttons, etc.).
  function toast(msg, ok) {
    var $t = $("#toast");
    if (!$t.length) {
      $t = $('<div id="toast" class="fixed bottom-4 right-4 z-50 space-y-2"></div>').appendTo("body");
    }
    var $el = $('<div class="px-3 py-2 rounded shadow text-white"></div>')
      .css("background-color", ok ? "#15803d" : "#b91c1c")
      .text(msg)
      .appendTo($t);
    setTimeout(function () { $el.fadeOut(300, function () { $el.remove(); }); }, 2500);
  }

  // Full-screen loading mask, shown automatically while AJAX is in flight.
  function ensureMask() {
    var $m = $("#loading-mask");
    if (!$m.length) {
      $m = $(
        '<div id="loading-mask" class="hidden fixed inset-0 z-50 bg-white/60 items-center justify-center">' +
          '<div class="h-8 w-8 rounded-full border-4 border-slate-300 border-t-slate-700 animate-spin"></div>' +
        "</div>"
      ).appendTo("body");
    }
    return $m;
  }
  function showLoading() { ensureMask().removeClass("hidden").addClass("flex"); }
  function hideLoading() { ensureMask().addClass("hidden").removeClass("flex"); }

  // Reusable confirmation modal; runs `onConfirm` only if the user confirms.
  function ensureConfirm() {
    var $m = $("#confirm-modal");
    if (!$m.length) {
      $m = $(
        '<div id="confirm-modal" class="modal hidden fixed inset-0 bg-black/40 z-50 items-center justify-center p-4">' +
          '<div class="bg-white rounded-lg shadow-lg w-full max-w-sm p-5">' +
            '<p id="confirm-message" class="mb-4"></p>' +
            '<div class="flex justify-end gap-2">' +
              '<button type="button" data-modal-close class="border rounded px-3 py-1.5">Cancel</button>' +
              '<button type="button" id="confirm-ok" class="bg-red-600 text-white rounded px-3 py-1.5 hover:bg-red-700">Confirm</button>' +
            "</div>" +
          "</div>" +
        "</div>"
      ).appendTo("body");
    }
    return $m;
  }
  function confirmAction(message, onConfirm) {
    var $m = ensureConfirm();
    $m.find("#confirm-message").text(message);
    $m.find("#confirm-ok").off("click").on("click", function () {
      closeModal("#confirm-modal");
      onConfirm();
    });
    openModal("#confirm-modal");
  }

  // Shared styling so every table action renders as a real button.
  var BTN_BASE = "inline-flex items-center px-2.5 py-1 mr-1 text-xs font-medium rounded border ";
  var BTN_VARIANT = {
    default: "border-slate-300 text-slate-700 hover:bg-slate-100",
    danger: "border-red-300 text-red-700 hover:bg-red-50",
    warn: "border-amber-300 text-amber-700 hover:bg-amber-50",
    success: "border-green-300 text-green-700 hover:bg-green-50",
  };

  function attrString(attrs) {
    return Object.keys(attrs || {}).map(function (k) {
      return " " + k + '="' + attrs[k] + '"';
    }).join("");
  }

  // A table-action button; `attrs` become HTML attributes (e.g. data-act).
  function actionButton(label, attrs, variant) {
    var cls = BTN_BASE + (BTN_VARIANT[variant] || BTN_VARIANT.default);
    return '<button type="button" class="' + cls + '"' + attrString(attrs) + ">" + label + "</button>";
  }

  // A navigation action styled as a button (anchor for accessibility).
  function linkButton(label, href, variant) {
    var cls = BTN_BASE + (BTN_VARIANT[variant] || BTN_VARIANT.default);
    return '<a class="' + cls + '" href="' + href + '">' + label + "</a>";
  }

  // A small status pill for table cells.
  var BADGE_VARIANT = {
    default: "bg-slate-100 text-slate-700",
    success: "bg-green-100 text-green-700",
    danger: "bg-red-100 text-red-700",
    warn: "bg-amber-100 text-amber-700",
  };
  function badge(label, variant) {
    var cls = "inline-block px-2 py-0.5 rounded text-xs font-medium " +
      (BADGE_VARIANT[variant] || BADGE_VARIANT.default);
    return '<span class="' + cls + '">' + label + "</span>";
  }

  // Exposed for page scripts (e.g. close a modal after a successful save).
  window.PI = {
    openModal: openModal,
    closeModal: closeModal,
    toast: toast,
    actionButton: actionButton,
    linkButton: linkButton,
    badge: badge,
    showLoading: showLoading,
    hideLoading: hideLoading,
    confirm: confirmAction,
  };

  // Show the loading mask for any foreground AJAX (requests opting out with
  // `global: false`, e.g. background polling, are skipped).
  $(document).ajaxStart(showLoading).ajaxStop(hideLoading);

  $(function () {
    highlightNav();
    initTabs();
    initModals();
  });
})(jQuery);
