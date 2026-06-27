// Generic platform entity list: search, paginate, link to detail.
(function ($) {
  "use strict";

  var entity = JSON.parse($("#entity-name").text());

  // Columns to render per entity (key -> header).
  var LINKED_COLUMNS = [["name", "Name"], ["kind", "Kind"], ["version", "Version"], ["applications", "Apps"]];
  var COLUMNS = {
    applications: [["name", "Name"], ["app_type", "Type"], ["primary_language", "Language"],
      ["libraries", "Libs"], ["infrastructure", "Infra"], ["dependencies", "Deps"]],
    infrastructure: LINKED_COLUMNS,
    tools: LINKED_COLUMNS,
    "cloud-providers": LINKED_COLUMNS,
    services: LINKED_COLUMNS,
    platforms: LINKED_COLUMNS,
    external: LINKED_COLUMNS,
    libraries: [["name", "Name"], ["ecosystem", "Ecosystem"], ["versions", "Versions"], ["applications", "Apps"]],
    users: [["username", "Username"], ["email", "Email"], ["groups", "Groups"], ["applications", "Apps"]],
    groups: [["name", "Name"], ["members", "Members"], ["applications", "Apps"]],
  };
  var NAME_KEY = { users: "username", default: "name" };

  var state = { search: "", page: 1, total: 0, pageSize: 25, filters: {} };

  function header() {
    var $tr = $("#list-table thead tr").empty();
    COLUMNS[entity].forEach(function (c) { $tr.append('<th class="p-3">' + c[1] + "</th>"); });
  }

  function nameOf(row) { return row[NAME_KEY[entity] || NAME_KEY.default]; }

  // Build the filter dropdowns from the entity's facets (distinct values per
  // filterable field). Selecting a value reloads page 1 with that filter.
  function buildFilters() {
    $.getJSON("/api/platform/" + entity + "/facets").done(function (facets) {
      var $box = $("#filters").empty();
      Object.keys(facets || {}).forEach(function (field) {
        var values = facets[field] || [];
        if (!values.length) return;
        var $sel = $('<select class="border rounded px-2 py-2 text-sm" data-field="' + field + '"></select>')
          .append('<option value="">All ' + $("<div>").text(PI.pluralize(PI.humanize(field))).html() + "</option>");
        values.forEach(function (v) {
          $sel.append($("<option>").val(v).text(v));
        });
        $sel.on("change", function () {
          var val = $(this).val();
          if (val) state.filters[field] = val; else delete state.filters[field];
          state.page = 1;
          load();
        });
        $box.append($sel);
      });
    });
  }

  function render(items) {
    var $body = $("#list-table tbody").empty();
    if (!items.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="6">Nothing found.</td></tr>');
      return;
    }
    items.forEach(function (row) {
      var $tr = $('<tr class="border-b hover:bg-slate-50 cursor-pointer"></tr>');
      COLUMNS[entity].forEach(function (c, i) {
        var v = row[c[0]];
        var empty = (v === null || v === undefined || v === "");
        var cell;
        if (i === 0) {
          cell = '<a class="text-blue-600" href="/platform/' + entity + "/" + row.id + '">' +
            $("<div>").text(empty ? "—" : v).html() + "</a>";
        } else if (empty) {
          cell = "—";
        } else if (PI.isBadgeKey(c[0])) {
          cell = PI.badgeFor(v);
        } else {
          cell = $("<div>").text(v).html();
        }
        $tr.append('<td class="p-3">' + cell + "</td>");
      });
      $body.append($tr);
    });
  }

  function load() {
    var params = $.extend({ search: state.search, page: state.page }, state.filters);
    $.getJSON("/api/platform/" + entity, params)
      .done(function (d) {
        state.total = d.total;
        state.pageSize = d.page_size;
        render(d.items);
        $("#total").text(d.total + " total");
        var pages = Math.max(1, Math.ceil(d.total / d.page_size));
        $("#page-info").text("Page " + state.page + " / " + pages);
        $("#prev").prop("disabled", state.page <= 1);
        $("#next").prop("disabled", state.page >= pages);
      });
  }

  $(function () {
    header();
    $("#pager").html(PI.paginationControls({ prev: 'id="prev"', page: 'id="page-info"', next: 'id="next"' }));
    buildFilters();
    load();
    var t;
    $("#search").on("input", function () {
      clearTimeout(t);
      var v = $(this).val();
      t = setTimeout(function () { state.search = v; state.page = 1; load(); }, 250);
    });
    $("#prev").on("click", function () { if (state.page > 1) { state.page--; load(); } });
    $("#next").on("click", function () { state.page++; load(); });
  });
})(jQuery);
