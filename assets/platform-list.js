// Generic platform entity list: search, paginate, link to detail.
(function ($) {
  "use strict";

  var entity = JSON.parse($("#entity-name").text());

  // Columns to render per entity (key -> header).
  var COLUMNS = {
    applications: [["name", "Name"], ["app_type", "Type"], ["primary_language", "Language"],
      ["libraries", "Libs"], ["infrastructure", "Infra"], ["dependencies", "Deps"]],
    infrastructure: [["name", "Name"], ["kind", "Kind"], ["version", "Version"], ["applications", "Apps"]],
    libraries: [["name", "Name"], ["ecosystem", "Ecosystem"], ["versions", "Versions"], ["applications", "Apps"]],
    users: [["username", "Username"], ["email", "Email"], ["groups", "Groups"], ["applications", "Apps"]],
    groups: [["name", "Name"], ["members", "Members"], ["applications", "Apps"]],
  };
  var NAME_KEY = { users: "username", default: "name" };

  var state = { search: "", page: 1, total: 0, pageSize: 25 };

  function header() {
    var $tr = $("#list-table thead tr").empty();
    COLUMNS[entity].forEach(function (c) { $tr.append('<th class="p-3">' + c[1] + "</th>"); });
  }

  function nameOf(row) { return row[NAME_KEY[entity] || NAME_KEY.default]; }

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
        if (v === null || v === undefined) v = "—";
        var cell = (i === 0)
          ? '<a class="text-blue-600" href="/platform/' + entity + "/" + row.id + '">' + $("<div>").text(v).html() + "</a>"
          : $("<div>").text(v).html();
        $tr.append('<td class="p-3">' + cell + "</td>");
      });
      $body.append($tr);
    });
  }

  function load() {
    $.getJSON("/api/platform/" + entity, { search: state.search, page: state.page })
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
