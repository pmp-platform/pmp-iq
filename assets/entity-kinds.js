// Settings → Entity kinds: manage the allowed kind vocabulary per entity type.
// Each kind has a stable id, a friendly name, and a description. Diagram and
// observability-signal kinds also carry a free-form JSON config.
(function ($) {
  "use strict";

  // Kind-bearing entity types, in display order.
  var ENTITIES = [
    ["applications", "Applications (app type)"],
    ["libraries", "Libraries (ecosystem)"],
    ["infrastructure", "Infrastructure"],
    ["tools", "Tools"],
    ["cloud-providers", "Cloud providers"],
    ["services", "Services"],
    ["platforms", "Platforms"],
    ["external", "External"],
    ["components", "Components"],
    ["diagrams", "Diagrams"],
    ["observability-signals", "Observability signals"],
  ];

  // Entity types whose kinds carry a JSON config object.
  var CONFIG_ENTITIES = { diagrams: true, "observability-signals": true };

  function esc(v) { return $("<div>").text(v === null || v === undefined ? "" : v).html(); }

  function configText(k) {
    if (!k.config || (typeof k.config === "object" && !Object.keys(k.config).length)) return "";
    return typeof k.config === "object" ? JSON.stringify(k.config) : String(k.config);
  }

  function row(k, hasConfig) {
    var cells =
      '<td class="py-1 pr-3 font-mono text-xs">' + esc(k.kind_id) + "</td>" +
      '<td class="py-1 pr-3">' + esc(k.name) + "</td>" +
      '<td class="py-1 pr-3 text-slate-500">' + esc(k.description) + "</td>";
    if (hasConfig) {
      cells += '<td class="py-1 pr-3 font-mono text-xs text-slate-500">' + esc(configText(k)) + "</td>";
    }
    cells += '<td class="py-1 text-right">' +
      '<button type="button" class="text-slate-400 hover:text-red-600" data-del="' + k.id + '">✕</button>' +
      "</td>";
    return '<tr class="border-b last:border-0">' + cells + "</tr>";
  }

  function section(entity, label, kinds) {
    var hasConfig = !!CONFIG_ENTITIES[entity];
    var head = '<th class="py-1 pr-3">Id</th><th class="py-1 pr-3">Name</th><th class="py-1 pr-3">Description</th>' +
      (hasConfig ? '<th class="py-1 pr-3">Config</th>' : "") + "<th></th>";
    var cols = hasConfig ? 5 : 4;
    var body = kinds.length
      ? kinds.map(function (k) { return row(k, hasConfig); }).join("")
      : '<tr><td class="py-1 text-slate-400" colspan="' + cols + '">No values.</td></tr>';
    var form =
      '<input data-kind-id placeholder="id (e.g. api)" class="border rounded px-2 py-1 text-sm w-40" />' +
      '<input data-kind-name placeholder="Friendly name" class="border rounded px-2 py-1 text-sm w-44" />' +
      '<input data-kind-desc placeholder="Description" class="border rounded px-2 py-1 text-sm w-56" />' +
      (hasConfig ? '<input data-kind-config placeholder="config JSON (optional)" class="border rounded px-2 py-1 text-sm font-mono w-56" />' : "") +
      '<button type="button" data-add class="bg-blue-100 text-blue-700 rounded px-3 py-1 text-sm font-medium hover:bg-blue-200">Add</button>';
    return '<div class="bg-white rounded-lg shadow border border-slate-200 p-4" data-entity="' + entity + '">' +
      '<h2 class="text-base font-semibold mb-2">' + esc(label) + "</h2>" +
      '<table class="w-full text-sm mb-2"><thead class="text-left text-slate-500 border-b"><tr>' + head +
      "</tr></thead><tbody>" + body + "</tbody></table>" +
      '<div class="flex flex-wrap gap-2 items-center">' + form + "</div></div>";
  }

  function render(kinds) {
    var byEntity = {};
    kinds.forEach(function (k) { (byEntity[k.entity_type] = byEntity[k.entity_type] || []).push(k); });
    var html = ENTITIES.map(function (e) { return section(e[0], e[1], byEntity[e[0]] || []); }).join("");
    $("#kinds-root").html(html);
  }

  function load() {
    $.getJSON("/api/settings/entity-kinds").done(function (d) { render(d.kinds); });
  }

  function add($card) {
    var entity = $card.data("entity");
    var kindId = $.trim($card.find("[data-kind-id]").val());
    var name = $.trim($card.find("[data-kind-name]").val());
    var description = $.trim($card.find("[data-kind-desc]").val());
    if (!kindId || !name) {
      window.PI.toast("Id and name are required", false);
      return;
    }
    var config = {};
    var $config = $card.find("[data-kind-config]");
    if ($config.length && $.trim($config.val())) {
      try { config = JSON.parse($config.val()); }
      catch (e) { window.PI.toast("Config must be valid JSON", false); return; }
    }
    $.ajax({
      url: "/api/settings/entity-kinds",
      method: "POST",
      contentType: "application/json",
      data: JSON.stringify({ entity_type: entity, kind_id: kindId, name: name, description: description, config: config }),
    }).done(load).fail(function (x) { window.PI.toast("Error: " + x.responseText, false); });
  }

  $(function () {
    if (!$("#kinds-root").length) return;
    $('<div class="flex justify-end mb-2"></div>')
      .append(window.PI.refreshButton(load))
      .insertBefore("#kinds-root");
    load();

    $("#kinds-root").on("click", "[data-add]", function () {
      add($(this).closest("[data-entity]"));
    });
    $("#kinds-root").on("click", "[data-del]", function () {
      var id = $(this).data("del");
      $.ajax({ url: "/api/settings/entity-kinds/" + id, method: "DELETE" })
        .done(load)
        .fail(function (x) { window.PI.toast("Error: " + x.responseText, false); });
    });
  });
})(jQuery);
