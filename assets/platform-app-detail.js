// Application detail page: a properties + languages header, then each relation
// rendered as a local searchable/filterable/paginated table (PI.localTable).
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "—" : v).html();
  }

  function prop(label, value) {
    return propRaw(label, esc(value));
  }

  function propRaw(label, valueHtml) {
    return '<div class="flex gap-2 text-sm py-0.5"><span class="w-36 text-slate-500">' +
      label + "</span><span>" + valueHtml + "</span></div>";
  }

  function renderProps(d) {
    var html = '<h2 class="text-base font-semibold mb-2">Properties</h2>';
    html += propRaw("Application Type", d.app_type ? PI.badgeFor(d.app_type) : esc(d.app_type));
    html += prop("Main Language", d.primary_language);
    html += prop("Description", d.description);
    if (d.metadata && typeof d.metadata === "object") {
      Object.keys(d.metadata).forEach(function (k) {
        var v = d.metadata[k];
        html += prop(k, typeof v === "object" ? JSON.stringify(v) : v);
      });
    }
    $("#app-props").html(html);
  }

  function renderLanguages(d) {
    var langs = d.languages || [];
    var html = '<h2 class="text-base font-semibold mb-2">Languages</h2>';
    if (!langs.length) {
      html += '<div class="text-sm text-slate-400">None detected.</div>';
    }
    langs.forEach(function (l) {
      var pct = l.percentage != null ? l.percentage + "%" : "—";
      html += '<div class="flex justify-between text-sm py-0.5"><span>' + esc(l.name) +
        '</span><span class="text-slate-500">' + esc(pct) + "</span></div>";
    });
    $("#app-languages").html(html);
  }

  function entityLink(entity) {
    return function (row) { return row.id ? "/platform/" + entity + "/" + row.id : null; };
  }

  // Linked relations rendered with the standard name/kind/version/usage columns.
  var LINKED = [
    ["infrastructure", "Infrastructure"],
    ["tools", "Tools"],
    ["cloud-providers", "Cloud providers"],
    ["services", "Services"],
    ["platforms", "Platforms"],
    ["external", "External"],
  ];

  // Render a relation table only when it has rows (keeps the page uncluttered).
  function table($root, opts) {
    if (!opts.rows || !opts.rows.length) return;
    opts.mount = $("<div></div>").appendTo($root);
    PI.localTable(opts);
  }

  function renderRelations(d) {
    var $root = $("#app-relations").empty();

    table($root, {
      title: "Libraries", rows: d.libraries || [],
      columns: [["name", "Name"], ["ecosystem", "Ecosystem"], ["version", "Version"], ["scope", "Scope"]],
      filterKey: "ecosystem", link: entityLink("libraries"),
    });
    table($root, {
      title: "Dependencies", rows: d.dependencies || [],
      columns: [["target_name", "Name"], ["kind", "Kind"], ["component_name", "Component"], ["description", "Description"]],
      filterKeys: ["kind", "component_name"],
      link: function (row) { return row.target_app_id ? "/platform/applications/" + row.target_app_id : null; },
    });
    LINKED.forEach(function (t) {
      table($root, {
        title: t[1], rows: d[t[0]] || [],
        columns: [["name", "Name"], ["kind", "Kind"], ["version", "Version"], ["usage", "Usage"]],
        filterKey: "kind", link: entityLink(t[0]),
      });
    });
    table($root, {
      title: "Components", rows: d.components || [],
      columns: [["name", "Name"], ["kind", "Kind"], ["description", "Description"]],
      filterKey: "kind",
    });
    var signals = [];
    (d.components || []).forEach(function (c) {
      (c.observability_signals || []).forEach(function (s) {
        signals.push({ name: s.name, kind: s.kind, component: c.name, description: s.description });
      });
    });
    table($root, {
      title: "Observability signals", rows: signals,
      columns: [["name", "Signal"], ["kind", "Kind"], ["component", "Component"], ["description", "Description"]],
      filterKeys: ["kind", "component"],
    });
    table($root, {
      title: "Access & members",
      rows: (d.access || []).map(function (a) {
        return $.extend({}, a, { permissions: permsSummary(a.permissions) });
      }),
      columns: [["principal_name", "Principal"], ["association_type", "Association"],
        ["access_level", "Level"], ["permissions", "Permissions"], ["principal_type", "Type"]],
      filterKeys: ["association_type", "principal_type"],
    });
  }

  // Use cases are rendered as cards (title, description, component chips) with
  // their mermaid diagrams rendered to SVG below.
  var diagramSeq = 0;
  function renderDiagram($card, diagram) {
    var $wrap = $('<div class="border border-slate-200 rounded p-3 my-2 bg-slate-50"></div>').appendTo($card);
    $wrap.append('<div class="text-sm font-medium mb-1">' + esc(diagram.name) +
      " " + (diagram.kind ? PI.badgeFor(diagram.kind) : "") + "</div>");
    if (diagram.description) {
      $wrap.append('<div class="text-xs text-slate-500 mb-2">' + esc(diagram.description) + "</div>");
    }
    var $svg = $('<div class="overflow-auto"></div>').appendTo($wrap);
    var source = diagram.content || "";
    function fail(msg) {
      $svg.html('<pre class="text-xs text-red-600 whitespace-pre-wrap">' +
        esc(msg) + "\n\n" + esc(source) + "</pre>");
    }
    if (typeof mermaid === "undefined" || !source.trim()) { fail("Diagram unavailable"); return; }
    try {
      mermaid.render("mmd-" + (++diagramSeq), source)
        .then(function (res) { $svg.html(res.svg); })
        .catch(function (err) { fail("Diagram failed to render: " + (err && err.message)); });
    } catch (err) {
      fail("Diagram failed to render: " + (err && err.message));
    }
  }

  function renderUseCases(d) {
    var useCases = d.use_cases || [];
    var $root = $("#app-use-cases").empty();
    if (!useCases.length) return;
    $root.append('<h2 class="text-base font-semibold mb-2">Use cases</h2>');
    useCases.forEach(function (uc) {
      var $card = $('<div class="bg-white rounded-lg shadow border border-slate-200 p-4 mb-3"></div>').appendTo($root);
      $card.append('<h3 class="font-semibold">' + esc(uc.name) + "</h3>");
      if (uc.description) {
        $card.append('<p class="text-sm text-slate-600 my-1">' + esc(uc.description) + "</p>");
      }
      if (uc.components && uc.components.length) {
        var chips = uc.components.map(function (c) {
          return '<span class="inline-block bg-slate-100 rounded px-2 py-0.5 text-xs mr-1 mb-1">' + esc(c.name) + "</span>";
        }).join("");
        $card.append('<div class="my-1">' + chips + "</div>");
      }
      (uc.diagrams || []).forEach(function (dg) { renderDiagram($card, dg); });
    });
  }

  // Summarise a provider permissions object ({admin:true,push:true,…}) to the
  // list of granted permissions.
  function permsSummary(perms) {
    if (!perms || typeof perms !== "object") return "";
    return Object.keys(perms).filter(function (k) { return perms[k] === true; }).join(", ");
  }

  function render(d) {
    $("#detail-title").text(d.name || "Application");
    renderProps(d);
    renderLanguages(d);
    renderRelations(d);
    renderUseCases(d);
  }

  $(function () {
    if (typeof mermaid !== "undefined") {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
    }
    $.getJSON("/api/platform/" + meta.entity + "/" + meta.id)
      .done(function (d) { render(d.detail); })
      .fail(function () { $("#detail-title").text("Not found"); });
  });
})(jQuery);
