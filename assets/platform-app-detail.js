// Application detail page: a tabbed view. Overview pairs a focused connection
// graph (the app + its dependencies + infrastructure) with its properties and
// languages; Use cases drives two G6 flowcharts (use cases → components) plus
// the AI mermaid diagrams; the remaining tabs are per-relation tables. Tabs that
// would be empty (services, tools, …) are omitted; Members is always present.
(function ($) {
  "use strict";

  var meta = JSON.parse($("#detail-meta").text());

  // Entity-kind colours, shared with the platform-wide graph legend.
  var COLORS = {
    application: "#2563eb",
    infrastructure: "#059669",
    tools: "#d97706",
    "cloud-providers": "#7c3aed",
    services: "#db2777",
    platforms: "#0891b2",
    external: "#9ca3af",
  };

  // Friendly property names from Settings → Properties (prop_id → name), with a
  // humanize() fallback for anything not configured.
  var propNames = {};
  function buildPropMap(props) {
    (props || []).forEach(function (p) {
      if (p.entity_type === "applications") propNames[p.prop_id] = p.name;
    });
  }
  function friendly(key) { return propNames[key] || PI.humanize(key); }

  function esc(v) {
    return $("<div>").text(v === null || v === undefined ? "—" : v).html();
  }

  // ---- Overview: properties + languages -----------------------------------

  function propRaw(label, valueHtml) {
    return '<div class="flex gap-2 text-sm py-0.5"><span class="w-36 shrink-0 text-slate-500">' +
      esc(label) + "</span><span>" + valueHtml + "</span></div>";
  }
  function prop(label, value) { return propRaw(label, esc(value)); }

  function propsHtml(d) {
    var html = '<h2 class="text-base font-semibold mb-2">Properties</h2>';
    html += propRaw("Application Type", d.app_type ? PI.badgeFor(d.app_type) : "—");
    html += prop("Main Language", d.primary_language);
    html += prop("Description", d.description);
    if (d.metadata && typeof d.metadata === "object") {
      Object.keys(d.metadata).forEach(function (k) {
        var v = d.metadata[k];
        html += prop(friendly(k), typeof v === "object" ? JSON.stringify(v) : v);
      });
    }
    return html;
  }

  function languagesHtml(d) {
    var langs = d.languages || [];
    var html = '<h2 class="text-base font-semibold mb-2">Languages</h2>';
    if (!langs.length) return html + '<div class="text-sm text-slate-400">None detected.</div>';
    langs.forEach(function (l) {
      var pct = l.percentage != null ? l.percentage + "%" : "—";
      html += '<div class="flex justify-between text-sm py-0.5"><span>' + esc(l.name) +
        '</span><span class="text-slate-500">' + esc(pct) + "</span></div>";
    });
    return html;
  }

  // ---- G6 graphs ----------------------------------------------------------

  function noG6($mount) {
    $mount.html('<div class="text-sm text-red-600 p-4">Graph library unavailable.</div>');
  }

  // Force-directed graph for the Overview: the app plus its dependencies and
  // infrastructure. Nodes carry an href so clicks drill into related entities.
  function overviewGraph(containerId, d) {
    if (typeof G6 === "undefined") return noG6($("#" + containerId));
    var nodes = [{ id: "app:" + d.id, data: { label: d.name, kind: "application" } }];
    var seen = {}; seen["app:" + d.id] = true;
    var edges = [];
    (d.dependencies || []).forEach(function (dep, i) {
      var resolved = !!dep.target_app_id;
      var id = resolved ? "app:" + dep.target_app_id : "dep:" + dep.target_name;
      if (!seen[id]) {
        seen[id] = true;
        nodes.push({ id: id, data: { label: dep.target_name, kind: resolved ? "application" : "external",
          href: resolved ? "/platform/applications/" + dep.target_app_id : null } });
      }
      edges.push({ id: "de" + i, source: "app:" + d.id, target: id, data: { kind: dep.kind || "" } });
    });
    (d.infrastructure || []).forEach(function (inf, i) {
      var id = "infra:" + inf.id;
      if (!seen[id]) {
        seen[id] = true;
        nodes.push({ id: id, data: { label: inf.name, kind: "infrastructure",
          href: "/platform/infrastructure/" + inf.id } });
      }
      edges.push({ id: "ie" + i, source: "app:" + d.id, target: id, data: { kind: inf.usage || "uses" } });
    });

    var graph = new G6.Graph({
      container: containerId,
      autoResize: true,
      autoFit: "view",
      data: { nodes: nodes, edges: edges },
      layout: { type: "d3-force", link: { distance: 120 }, manyBody: { strength: -320 }, collide: { radius: 30 } },
      node: {
        style: {
          size: 16,
          fill: function (n) { return COLORS[n.data.kind] || "#64748b"; },
          stroke: "#ffffff", lineWidth: 1,
          labelText: function (n) { return n.data.label; },
          labelPlacement: "bottom", labelFontSize: 9, labelFill: "#0f172a",
        },
      },
      edge: {
        style: {
          stroke: "#cbd5e1", endArrow: true,
          labelText: function (e) { return e.data.kind; },
          labelFontSize: 7, labelFill: "#94a3b8",
        },
      },
      behaviors: ["zoom-canvas", "drag-canvas", "drag-element"],
    });
    graph.on("node:click", function (evt) {
      var id = evt.target && evt.target.id;
      if (!id) return;
      var n = graph.getNodeData(id);
      if (n && n.data && n.data.href) window.location.href = n.data.href;
    });
    graph.render();
    return graph;
  }

  // Top-down rectangular flowchart (AntV "System Performance Diagnosis"-style).
  function flowchart(containerId, nodes, edges, onClick) {
    if (typeof G6 === "undefined") { noG6($("#" + containerId)); return null; }
    var graph = new G6.Graph({
      container: containerId,
      autoResize: true,
      autoFit: "view",
      data: { nodes: nodes, edges: edges },
      node: {
        type: "rect",
        style: {
          size: [170, 42], radius: 6,
          fill: function (n) { return n.data.fill || "#eff6ff"; },
          stroke: "#94a3b8", lineWidth: 1,
          labelText: function (n) { return n.data.label; },
          labelPlacement: "center", labelFontSize: 11, labelFill: "#0f172a",
        },
      },
      edge: { type: "polyline", style: { stroke: "#94a3b8", endArrow: true } },
      layout: { type: "antv-dagre", rankdir: "TB", nodesep: 24, ranksep: 44 },
      behaviors: ["zoom-canvas", "drag-canvas", "drag-element"],
    });
    if (onClick) {
      graph.on("node:click", function (evt) {
        var id = evt.target && evt.target.id;
        if (!id) return;
        var n = graph.getNodeData(id);
        if (n && n.data) onClick(n.data);
      });
    }
    graph.render();
    return graph;
  }

  // ---- Use cases ----------------------------------------------------------

  var diagramSeq = 0;
  function renderDiagram($root, diagram) {
    var $wrap = $('<div class="border border-slate-200 rounded p-3 my-2 bg-slate-50"></div>').appendTo($root);
    $wrap.append('<div class="text-sm font-medium mb-1">' + esc(diagram.name) +
      " " + (diagram.kind ? PI.badgeFor(diagram.kind) : "") + "</div>");
    if (diagram.description) {
      $wrap.append('<div class="text-xs text-slate-500 mb-2">' + esc(diagram.description) + "</div>");
    }
    var $svg = $('<div class="overflow-auto"></div>').appendTo($wrap);
    var source = diagram.content || "";
    function fail(msg) {
      $svg.html('<pre class="text-xs text-red-600 whitespace-pre-wrap">' + esc(msg) + "\n\n" + esc(source) + "</pre>");
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

  function renderUseCases($panel, d) {
    var useCases = d.use_cases || [];
    if (!useCases.length) {
      $panel.html('<div class="text-sm text-slate-400">No use cases.</div>');
      return;
    }
    $panel.html(
      '<div class="bg-white rounded-lg shadow border border-slate-200 p-2 mb-4">' +
        '<div class="text-xs text-slate-400 px-2 pt-1">Click a use case to see its components.</div>' +
        '<div id="uc-flow" style="height:340px"></div></div>' +
      '<div id="uc-detail"></div>'
    );

    var compGraph = null;
    function selectUseCase(uc) {
      $("#uc-detail").html(
        '<h3 class="font-semibold mb-1">' + esc(uc.name) + "</h3>" +
        (uc.description ? '<p class="text-sm text-slate-600 mb-2">' + esc(uc.description) + "</p>" : "") +
        '<div class="bg-white rounded-lg shadow border border-slate-200 p-2">' +
          '<div id="uc-comp" style="height:320px"></div></div>' +
        '<div id="uc-diagrams" class="mt-3"></div>'
      );
      if (compGraph) { try { compGraph.destroy(); } catch (e) { /* ignore */ } compGraph = null; }
      var comps = uc.components || [];
      if (comps.length) {
        var n = [{ id: "uc", data: { label: uc.name, fill: "#dbeafe" } }];
        var e = [];
        comps.forEach(function (c, i) {
          n.push({ id: "c" + i, data: { label: c.name, fill: "#f1f5f9" } });
          e.push({ id: "ce" + i, source: "uc", target: "c" + i });
        });
        compGraph = flowchart("uc-comp", n, e, null);
      } else {
        $("#uc-comp").html('<div class="text-sm text-slate-400 p-4">No components linked.</div>');
      }
      var $dg = $("#uc-diagrams").empty();
      (uc.diagrams || []).forEach(function (dg) { renderDiagram($dg, dg); });
    }

    var nodes = [{ id: "app", data: { label: d.name, fill: "#dbeafe" } }];
    var edges = [];
    useCases.forEach(function (uc, i) {
      nodes.push({ id: "uc" + i, data: { label: uc.name, fill: "#fef3c7", ucIndex: i } });
      edges.push({ id: "ue" + i, source: "app", target: "uc" + i });
    });
    flowchart("uc-flow", nodes, edges, function (data) {
      if (data.ucIndex != null) selectUseCase(useCases[data.ucIndex]);
    });
    selectUseCase(useCases[0]);
  }

  // ---- Tables -------------------------------------------------------------

  function entityLink(entity) {
    return function (row) { return row.id ? "/platform/" + entity + "/" + row.id : null; };
  }
  function permsSummary(perms) {
    if (!perms || typeof perms !== "object") return "";
    return Object.keys(perms).filter(function (k) { return perms[k] === true; }).join(", ");
  }

  // A tab whose body is a single PI.localTable.
  function tableRender(opts) {
    return function ($panel) {
      opts.mount = $('<div></div>').appendTo($panel.empty());
      PI.localTable(opts);
    };
  }

  // ---- Tab framework ------------------------------------------------------

  var TAB_BASE = "px-3 py-2 border-b-2 -mb-px text-sm";
  function setActive(tab, on) {
    tab.$btn.attr("class", TAB_BASE + (on
      ? " border-slate-900 text-slate-900 font-medium"
      : " border-transparent text-slate-500 hover:text-slate-700"));
    tab.$panel.toggleClass("hidden", !on);
  }

  function buildTabs(tabs) {
    var $root = $("#app-detail").empty();
    var $bar = $('<div class="border-b border-slate-200 mb-4 flex gap-1 flex-wrap"></div>').appendTo($root);
    var $panels = $("<div></div>").appendTo($root);
    function activate(tab) {
      tabs.forEach(function (t) { setActive(t, t === tab); });
      if (!tab.rendered) { tab.rendered = true; tab.render(tab.$panel); }
    }
    tabs.forEach(function (tab) {
      tab.$btn = $('<button type="button"></button>').text(tab.label).appendTo($bar);
      tab.$panel = $('<div class="hidden"></div>').appendTo($panels);
      tab.rendered = false;
      tab.$btn.on("click", function () { activate(tab); });
    });
    if (tabs.length) activate(tabs[0]);
  }

  function render(d) {
    var sub = "";
    if (d.app_type) sub += PI.badgeFor(d.app_type) + " ";
    if (d.description) sub += esc(d.description);
    $("#detail-title").text(d.name || "Application");
    $("#detail-subtitle").html(sub);

    var tabs = [];
    tabs.push({ label: "Overview", render: function ($p) {
      $p.html(
        '<div class="grid grid-cols-1 lg:grid-cols-3 gap-4">' +
          '<div class="lg:col-span-2 bg-white rounded-lg shadow border border-slate-200 p-2">' +
            '<div id="ov-graph" style="height:480px"></div></div>' +
          '<div class="bg-white rounded-lg shadow border border-slate-200 p-4">' +
            '<div id="ov-props"></div><div id="ov-langs" class="mt-4"></div></div>' +
        "</div>"
      );
      $("#ov-props").html(propsHtml(d));
      $("#ov-langs").html(languagesHtml(d));
      overviewGraph("ov-graph", d);
    } });

    tabs.push({ label: "Use cases", render: function ($p) { renderUseCases($p, d); } });

    // Conditional linked-entity / component tables (shown only when populated).
    var LINKED = [
      ["services", "Services"], ["cloud-providers", "Cloud providers"],
      ["platforms", "Platforms"], ["tools", "Tools"], ["external", "External"],
    ];
    function linkedTab(key, label) {
      if (!(d[key] || []).length) return;
      tabs.push({ label: label, render: tableRender({
        title: label, rows: d[key],
        columns: [["name", "Name"], ["kind", "Kind"], ["version", "Version"], ["usage", "Usage"]],
        filterKey: "kind", link: entityLink(key),
      }) });
    }
    linkedTab("services", "Services");
    linkedTab("cloud-providers", "Cloud providers");
    linkedTab("platforms", "Platforms");

    if ((d.libraries || []).length) {
      tabs.push({ label: "Libraries", render: tableRender({
        title: "Libraries", rows: d.libraries,
        columns: [["name", "Name"], ["ecosystem", "Ecosystem"], ["version", "Version"], ["scope", "Scope"]],
        filterKey: "ecosystem", link: entityLink("libraries"),
      }) });
    }
    linkedTab("tools", "Tools");
    linkedTab("external", "External");

    if ((d.components || []).length) {
      tabs.push({ label: "Components", render: tableRender({
        title: "Components", rows: d.components,
        columns: [["name", "Name"], ["kind", "Kind"], ["description", "Description"]],
        filterKey: "kind",
      }) });
    }

    var signals = [];
    (d.components || []).forEach(function (c) {
      (c.observability_signals || []).forEach(function (s) {
        signals.push({ name: s.name, kind: s.kind, component: c.name, description: s.description });
      });
    });
    if (signals.length) {
      tabs.push({ label: "Observability", render: tableRender({
        title: "Observability signals", rows: signals,
        columns: [["name", "Signal"], ["kind", "Kind"], ["component", "Component"], ["description", "Description"]],
        filterKeys: ["kind", "component"],
      }) });
    }

    tabs.push({ label: "Members", render: tableRender({
      title: "Members", rows: (d.access || []).map(function (a) {
        return $.extend({}, a, { permissions: permsSummary(a.permissions) });
      }),
      columns: [["principal_name", "Principal"], ["association_type", "Association"],
        ["access_level", "Level"], ["permissions", "Permissions"], ["principal_type", "Type"]],
      filterKeys: ["association_type", "principal_type"],
    }) });

    buildTabs(tabs);
  }

  $(function () {
    if (typeof mermaid !== "undefined") {
      mermaid.initialize({ startOnLoad: false, securityLevel: "strict" });
    }
    // Friendly property names are best-effort; render regardless of the result.
    $.getJSON("/api/settings/entity-properties")
      .always(function (p) { buildPropMap(p && p.properties); })
      .always(function () {
        $.getJSON("/api/platform/" + meta.entity + "/" + meta.id)
          .done(function (d) { render(d.detail); })
          .fail(function () { $("#detail-title").text("Not found"); });
      });
  });
})(jQuery);
