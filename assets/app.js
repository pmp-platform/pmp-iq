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
  // [data-modal-close] or Escape. Clicking the backdrop does NOT close the
  // modal, to avoid discarding in-progress edits by accident.
  function initModals() {
    $(document).on("click", "[data-modal-open]", function () {
      openModal("#" + $(this).data("modal-open"));
    });
    $(document).on("click", "[data-modal-close]", function () {
      closeModal($(this).closest(".modal"));
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
              '<button type="button" data-modal-close class="btn btn-danger btn-sm">Cancel</button>' +
              '<button type="button" id="confirm-ok" class="btn btn-primary btn-sm">Confirm</button>' +
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

  // Shared styling so every table action renders as a real button — compact and
  // light colour-filled.
  var BTN_BASE = "inline-flex items-center gap-1 px-2 py-0.5 mr-1 text-xs font-medium rounded ";
  var BTN_VARIANT = {
    default: "bg-slate-100 text-slate-700 hover:bg-slate-200",
    primary: "bg-blue-100 text-blue-700 hover:bg-blue-200",
    danger: "bg-red-100 text-red-700 hover:bg-red-200",
    warn: "bg-amber-100 text-amber-700 hover:bg-amber-200",
    success: "bg-green-100 text-green-700 hover:bg-green-200",
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

  // A "Refresh" toolbar button that re-runs `onClick` (a section's reload
  // function). Returns a jQuery element so callers drop it into a section's
  // toolbar; sized to match the "Add …" toolbar buttons.
  function refreshButton(onClick) {
    var $btn = $(
      '<button type="button" title="Refresh" ' +
        'class="btn btn-secondary btn-sm">' +
        "↻ Refresh</button>"
    );
    $btn.on("click", onClick);
    return $btn;
  }

  // A small status pill for table cells.
  var BADGE_VARIANT = {
    default: "bg-slate-100 text-slate-700",
    neutral: "bg-slate-100 text-slate-600",
    success: "bg-green-100 text-green-700",
    danger: "bg-red-100 text-red-700",
    warn: "bg-amber-100 text-amber-700",
    info: "bg-blue-100 text-blue-700",
    purple: "bg-purple-100 text-purple-700",
    teal: "bg-teal-100 text-teal-700",
    indigo: "bg-indigo-100 text-indigo-700",
    cyan: "bg-cyan-100 text-cyan-700",
    pink: "bg-pink-100 text-pink-700",
  };
  function badge(label, variant) {
    var cls = "inline-block px-2 py-0.5 rounded text-xs font-medium " +
      (BADGE_VARIANT[variant] || BADGE_VARIANT.default);
    return '<span class="' + cls + '">' + label + "</span>";
  }

  function escText(v) {
    return $("<div>").text(v === null || v === undefined ? "" : v).html();
  }

  // Well-known enum values get a semantic colour; any other categorical value
  // gets a stable, non-semantic colour derived from the value itself.
  var BADGE_SEMANTIC = {
    queued: "neutral", running: "info", paused: "warn", succeeded: "success",
    completed: "success", failed: "danger", cancelled: "warn", error: "danger",
    member: "success", ex_member: "danger", codeowner: "info",
    manual: "neutral", cron: "info", scheduled: "info", resume: "warn",
    yes: "success", no: "neutral",
  };
  var BADGE_PALETTE = ["info", "purple", "teal", "indigo", "cyan", "pink"];
  function paletteVariant(s) {
    var h = 0;
    for (var i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
    return BADGE_PALETTE[h % BADGE_PALETTE.length];
  }

  // Render a categorical value as a coloured badge (escaped).
  function badgeFor(value) {
    var key = String(value).toLowerCase();
    return badge(escText(value), BADGE_SEMANTIC[key] || paletteVariant(key));
  }

  // Column keys whose values are categorical and should render as badges.
  var BADGE_KEYS = {
    status: 1, trigger: 1, trigger_type: 1, association_type: 1,
    principal_type: 1, kind: 1, ecosystem: 1, app_type: 1, scope: 1,
  };
  function isBadgeKey(key) {
    return Object.prototype.hasOwnProperty.call(BADGE_KEYS, key);
  }

  // Shared, right-aligned pagination controls reused by every paginated table.
  // `ids` supplies the button/label attribute fragments, e.g.
  // { prev: 'id="prev"', page: 'id="page-info"', next: 'id="next"' } or the
  // data-attribute variants used by the in-memory tables.
  var PAGER_WRAP = "flex items-center justify-end gap-1 mt-3 text-sm";
  var PAGER_BTN = "px-2 py-0.5 text-xs rounded-md border border-slate-300 bg-white text-slate-700 " +
    "hover:bg-slate-50 transition-colors disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:bg-white";
  var PAGER_INFO = "px-3 text-slate-500 tabular-nums";
  function paginationControls(ids) {
    return '<div class="' + PAGER_WRAP + '">' +
      "<button " + ids.prev + ' class="' + PAGER_BTN + '">Prev</button>' +
      "<span " + ids.page + ' class="' + PAGER_INFO + '"></span>' +
      "<button " + ids.next + ' class="' + PAGER_BTN + '">Next</button>' +
      "</div>";
  }

  // "language_version" / "primary-language" -> "Language Version".
  function humanize(key) {
    return String(key).replace(/[_-]+/g, " ").replace(/\b\w/g, function (c) { return c.toUpperCase(); });
  }

  // Pluralise the last word of a label: "App Type" -> "App Types",
  // "Category" -> "Categories", "Status" -> "Statuses".
  function pluralize(label) {
    var parts = String(label).split(" ");
    var last = parts[parts.length - 1];
    if (!last) return label;
    if (/(s|x|z|ch|sh)$/i.test(last)) last += "es";
    else if (/[^aeiou]y$/i.test(last)) last = last.slice(0, -1) + "ies";
    else last += "s";
    parts[parts.length - 1] = last;
    return parts.join(" ");
  }

  // Exposed for page scripts (e.g. close a modal after a successful save).
  window.PI = {
    openModal: openModal,
    closeModal: closeModal,
    toast: toast,
    actionButton: actionButton,
    linkButton: linkButton,
    refreshButton: refreshButton,
    badge: badge,
    badgeFor: badgeFor,
    isBadgeKey: isBadgeKey,
    paginationControls: paginationControls,
    humanize: humanize,
    pluralize: pluralize,
    showLoading: showLoading,
    hideLoading: hideLoading,
    confirm: confirmAction,
  };

  // Show the loading mask for any foreground AJAX (requests opting out with
  // `global: false`, e.g. background polling, are skipped).
  $(document).ajaxStart(showLoading).ajaxStop(hideLoading);

  // Confirm before logging out, then submit the real logout form.
  function initLogout() {
    $(document).on("click", "#logout-btn", function () {
      confirmAction("Log out of pmp-iq?", function () {
        $("#logout-form").get(0).submit();
      });
    });
  }

  $(function () {
    highlightNav();
    initTabs();
    initModals();
    initLogout();
  });
})(jQuery);
