// Reusable in-memory table: title, search, filter dropdowns, pagination.
// Rows are sorted alphabetically by default; used by detail pages to render
// each relation locally.
(function ($) {
  "use strict";

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "" : v).html();
  }

  // Distinct, sorted, non-empty values of `key` across rows (for a filter).
  function distinct(rows, key) {
    var seen = {};
    var out = [];
    rows.forEach(function (r) {
      var v = r[key];
      if (v !== null && v !== undefined && v !== "" && !seen[v]) {
        seen[v] = true;
        out.push(v);
      }
    });
    return out.sort();
  }

  // Normalise the (optional) single filterKey + (optional) filterKeys array
  // into one list of keys.
  function filterKeysOf(opts) {
    var keys = (opts.filterKeys || []).slice();
    if (opts.filterKey && keys.indexOf(opts.filterKey) < 0) keys.unshift(opts.filterKey);
    return keys;
  }

  function buildShell(opts, rows, keys) {
    var html = '<div class="bg-white rounded-lg shadow border border-slate-200 p-4">';
    html += '<div class="flex items-center justify-between mb-2 gap-2 flex-wrap">';
    html += '<h2 class="text-base font-semibold">' + esc(opts.title) +
      ' <span class="text-slate-400 font-normal" data-count></span></h2>';
    html += '<div class="flex items-center gap-2 flex-wrap">';
    html += '<input data-search placeholder="Search…" class="border rounded px-2 py-1 text-sm w-44" />';
    keys.forEach(function (key) {
      var values = distinct(rows, key);
      if (!values.length) return;
      html += '<select data-filter="' + esc(key) + '" class="border rounded px-2 py-1 text-sm">' +
        '<option value="">All ' + esc(window.PI.pluralize(window.PI.humanize(key))) + "</option>";
      values.forEach(function (o) {
        html += '<option value="' + esc(o) + '">' + esc(o) + "</option>";
      });
      html += "</select>";
    });
    html += "</div></div>";
    html += '<table class="w-full text-sm"><thead class="text-left text-slate-500 border-b"><tr>';
    opts.columns.forEach(function (c) { html += '<th class="py-2 pr-3">' + esc(c[1]) + "</th>"; });
    html += '</tr></thead><tbody data-body></tbody></table>';
    html += window.PI.paginationControls({ prev: "data-prev", page: "data-page", next: "data-next" });
    html += "</div>";
    return html;
  }

  function matches(row, columns, keys, state) {
    for (var i = 0; i < keys.length; i++) {
      var key = keys[i];
      if (state.filters[key] && String(row[key]) !== state.filters[key]) return false;
    }
    var s = state.search.toLowerCase();
    if (!s) return true;
    return columns.some(function (c) {
      var v = row[c[0]];
      return v !== null && v !== undefined && String(v).toLowerCase().indexOf(s) >= 0;
    });
  }

  function rowHtml(row, columns, opts) {
    var cells = "";
    columns.forEach(function (c, i) {
      var raw = row[c[0]];
      var empty = (raw === null || raw === undefined || raw === "");
      var href = (i === 0 && opts.link) ? opts.link(row) : null;
      var cell;
      if (href) {
        cell = '<a class="text-blue-600" href="' + href + '">' + esc(empty ? "—" : raw) + "</a>";
      } else if (empty) {
        cell = "—";
      } else if (window.PI.isBadgeKey(c[0])) {
        cell = window.PI.badgeFor(raw);
      } else {
        cell = esc(raw);
      }
      cells += '<td class="py-2 pr-3">' + cell + "</td>";
    });
    return '<tr class="border-b last:border-0">' + cells + "</tr>";
  }

  // Sort rows alphabetically by `sortKey` (defaults to the first column),
  // unless opts.sort === false.
  function sortRows(rows, columns, opts) {
    if (opts.sort === false) return rows;
    var key = opts.sortKey || (columns[0] && columns[0][0]);
    if (!key) return rows;
    return rows.slice().sort(function (a, b) {
      return String(a[key] == null ? "" : a[key])
        .localeCompare(String(b[key] == null ? "" : b[key]), undefined, { sensitivity: "base" });
    });
  }

  // opts: { mount, title, rows, columns:[[key,label]], filterKey?, filterKeys?,
  //         sortKey?, sort?, link?(row)->href, pageSize? }
  function localTable(opts) {
    var $mount = $(opts.mount);
    if (!$mount.length) return;
    var columns = opts.columns || [];
    var rows = sortRows(opts.rows || [], columns, opts);
    var pageSize = opts.pageSize || 10;
    var keys = filterKeysOf(opts);
    var state = { search: "", filters: {}, page: 1 };

    $mount.html(buildShell(opts, rows, keys));
    var $body = $mount.find("[data-body]");

    function render() {
      var data = rows.filter(function (r) { return matches(r, columns, keys, state); });
      var pages = Math.max(1, Math.ceil(data.length / pageSize));
      if (state.page > pages) state.page = pages;
      var slice = data.slice((state.page - 1) * pageSize, state.page * pageSize);
      $body.empty();
      if (!slice.length) {
        $body.append('<tr><td class="py-2 text-slate-400" colspan="' + columns.length + '">Nothing found.</td></tr>');
      } else {
        slice.forEach(function (r) { $body.append(rowHtml(r, columns, opts)); });
      }
      $mount.find("[data-count]").text("(" + data.length + ")");
      $mount.find("[data-page]").text("Page " + state.page + " / " + pages);
      $mount.find("[data-prev]").prop("disabled", state.page <= 1);
      $mount.find("[data-next]").prop("disabled", state.page >= pages);
    }

    var t;
    $mount.find("[data-search]").on("input", function () {
      var v = $(this).val();
      clearTimeout(t);
      t = setTimeout(function () { state.search = v; state.page = 1; render(); }, 200);
    });
    $mount.find("[data-filter]").on("change", function () {
      state.filters[$(this).data("filter")] = $(this).val();
      state.page = 1;
      render();
    });
    $mount.find("[data-prev]").on("click", function () { if (state.page > 1) { state.page--; render(); } });
    $mount.find("[data-next]").on("click", function () { state.page++; render(); });
    render();
  }

  window.PI = window.PI || {};
  window.PI.localTable = localTable;
})(jQuery);
