// Settings → Properties: manage the properties the analyzer extracts into each
// entity's metadata.
(function ($) {
  "use strict";

  // Property-bearing entity types, in display order.
  var ENTITIES = [
    ["applications", "Applications"],
    ["libraries", "Libraries"],
    ["infrastructure", "Infrastructure"],
    ["tools", "Tools"],
    ["cloud-providers", "Cloud providers"],
    ["services", "Services"],
    ["platforms", "Platforms"],
    ["external", "External"],
    ["users", "Users"],
    ["groups", "Groups"],
    ["components", "Components"],
    ["use-cases", "Use cases"],
    ["diagrams", "Diagrams"],
    ["observability-signals", "Observability signals"],
  ];

  var DATA_TYPES = [
    ["string", "string"],
    ["number", "number"],
    ["boolean", "boolean"],
    ["date", "date"],
    ["array_of_strings", "array of strings"],
  ];

  function esc(v) { return $("<div>").text(v === null || v === undefined ? "" : v).html(); }

  function typeOptions() {
    return DATA_TYPES.map(function (t) {
      return '<option value="' + t[0] + '">' + esc(t[1]) + "</option>";
    }).join("");
  }

  function row(p) {
    return '<tr class="border-b last:border-0">' +
      '<td class="py-1 pr-3 font-mono text-xs">' + esc(p.prop_id) + "</td>" +
      '<td class="py-1 pr-3">' + esc(p.name) + "</td>" +
      '<td class="py-1 pr-3 text-slate-500">' + esc(p.description) + "</td>" +
      '<td class="py-1 pr-3 text-slate-500">' + esc(p.data_type) + "</td>" +
      '<td class="py-1 text-right">' +
        '<button type="button" class="text-slate-400 hover:text-red-600" data-del="' + p.id + '">✕</button>' +
      "</td></tr>";
  }

  function section(entity, label, props) {
    var body = props.length
      ? props.map(row).join("")
      : '<tr><td class="py-1 text-slate-400" colspan="5">No properties.</td></tr>';
    return '<div class="bg-white rounded-lg shadow border border-slate-200 p-4" data-entity="' + entity + '">' +
      '<h2 class="text-base font-semibold mb-2">' + esc(label) + "</h2>" +
      '<table class="w-full text-sm mb-2"><thead class="text-left text-slate-500 border-b"><tr>' +
        '<th class="py-1 pr-3">Id</th><th class="py-1 pr-3">Name</th><th class="py-1 pr-3">Description</th><th class="py-1 pr-3">Type</th><th></th>' +
      "</tr></thead><tbody>" + body + "</tbody></table>" +
      '<div class="flex flex-wrap gap-2 items-center">' +
        '<input data-prop-id placeholder="id (e.g. language_version)" class="border rounded px-2 py-1 text-sm w-52" />' +
        '<input data-prop-name placeholder="Friendly name" class="border rounded px-2 py-1 text-sm w-44" />' +
        '<input data-prop-desc placeholder="Description" class="border rounded px-2 py-1 text-sm w-56" />' +
        '<select data-prop-type class="border rounded px-2 py-1 text-sm">' + typeOptions() + "</select>" +
        '<button type="button" data-add class="bg-blue-100 text-blue-700 rounded px-3 py-1 text-sm font-medium hover:bg-blue-200">Add</button>' +
      "</div></div>";
  }

  function render(props) {
    var byEntity = {};
    props.forEach(function (p) { (byEntity[p.entity_type] = byEntity[p.entity_type] || []).push(p); });
    var html = ENTITIES.map(function (e) { return section(e[0], e[1], byEntity[e[0]] || []); }).join("");
    $("#properties-root").html(html);
  }

  function load() {
    $.getJSON("/api/settings/entity-properties").done(function (d) { render(d.properties); });
  }

  function add($card) {
    var entity = $card.data("entity");
    var propId = $.trim($card.find("[data-prop-id]").val());
    var name = $.trim($card.find("[data-prop-name]").val());
    var description = $.trim($card.find("[data-prop-desc]").val());
    var dataType = $card.find("[data-prop-type]").val();
    if (!propId || !name) {
      window.PI.toast("Id and name are required", false);
      return;
    }
    $.ajax({
      url: "/api/settings/entity-properties",
      method: "POST",
      contentType: "application/json",
      data: JSON.stringify({ entity_type: entity, prop_id: propId, name: name, description: description, data_type: dataType }),
    }).done(load).fail(function (x) { window.PI.toast("Error: " + x.responseText, false); });
  }

  $(function () {
    if (!$("#properties-root").length) return;
    load();

    $("#properties-root").on("click", "[data-add]", function () {
      add($(this).closest("[data-entity]"));
    });
    $("#properties-root").on("click", "[data-del]", function () {
      var id = $(this).data("del");
      $.ajax({ url: "/api/settings/entity-properties/" + id, method: "DELETE" })
        .done(load)
        .fail(function (x) { window.PI.toast("Error: " + x.responseText, false); });
    });
  });
})(jQuery);
