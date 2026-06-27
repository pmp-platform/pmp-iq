// Application detail page: a tabbed view. Overview pairs a focused connection
// graph (the app + its dependencies + infrastructure) with its properties and
// languages; Use cases shows a flowchart of use cases — clicking one opens a
// wide modal with a Components Diagram (G6) and a Flow Chart (mermaid); the
// remaining tabs are per-relation tables. Empty relation tabs are omitted;
// Members is always present. Every diagram has zoom/reset controls and never
// zooms on wheel scroll.
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

  // A small diagram control button (zoom in/out/reset).
  function ctrlBtn(label, title, fn) {
    return $('<button type="button" class="border border-slate-300 rounded px-2 py-0.5 text-xs text-slate-600 hover:bg-slate-100"></button>')
      .text(label).attr("title", title).on("click", fn);
  }

  // Wire zoom-in / zoom-out / fit controls to a G6 graph (wheel zoom is off, so
  // these buttons are the way to zoom).
  function attachG6Controls($bar, graph) {
    if (!graph) return;
    $bar.append(ctrlBtn("−", "Zoom out", function () { graph.zoomBy(0.8); }));
    $bar.append(ctrlBtn("+", "Zoom in", function () { graph.zoomBy(1.2); }));
    $bar.append(ctrlBtn("Reset", "Fit to view", function () { graph.fitView(); }));
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

  function noG6(container) {
    $(container).html('<div class="text-sm text-red-600 p-4">Graph library unavailable.</div>');
  }

  // Force-directed graph for the Overview: the app plus its dependencies and
  // infrastructure. Nodes carry an href so clicks drill into related entities.
  function overviewGraph(container, d) {
    if (typeof G6 === "undefined") { noG6(container); return null; }
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
      container: container,
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
      // No "zoom-canvas": wheel scroll never zooms; use the buttons instead.
      behaviors: ["drag-canvas", "drag-element"],
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
  function flowchart(container, nodes, edges, onClick) {
    if (typeof G6 === "undefined") { noG6(container); return null; }
    var graph = new G6.Graph({
      container: container,
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
      // No "zoom-canvas": wheel scroll never zooms; use the buttons instead.
      behaviors: ["drag-canvas", "drag-element"],
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

  // ---- Diagrams (mermaid) -------------------------------------------------

  var diagramSeq = 0;
  // Render a mermaid diagram into a bounded, scrollable, button-zoomable box so
  // large diagrams never spill out of their block.
  function renderDiagram($root, diagram) {
    var $wrap = $('<div class="border border-slate-200 rounded p-3 my-2 bg-slate-50"></div>').appendTo($root);
    $wrap.append('<div class="text-sm font-medium mb-1">' + esc(diagram.name) +
      " " + (diagram.kind ? PI.badgeFor(diagram.kind) : "") + "</div>");
    if (diagram.description) {
      $wrap.append('<div class="text-xs text-slate-500 mb-2">' + esc(diagram.description) + "</div>");
    }
    var $bar = $('<div class="flex gap-1 mb-2"></div>').appendTo($wrap);
    var $view = $('<div class="overflow-auto border border-slate-200 rounded bg-white" style="max-height:60vh"></div>').appendTo($wrap);
    var $inner = $('<div style="transform-origin:top left; display:inline-block; padding:8px;"></div>').appendTo($view);
    var source = diagram.content || "";

    var scale = 1;
    function apply() { $inner.css("transform", "scale(" + scale + ")"); }
    $bar.append(ctrlBtn("−", "Zoom out", function () { scale = Math.max(0.2, Math.round((scale - 0.2) * 10) / 10); apply(); }));
    $bar.append(ctrlBtn("+", "Zoom in", function () { scale = Math.min(4, Math.round((scale + 0.2) * 10) / 10); apply(); }));
    $bar.append(ctrlBtn("Reset", "Reset zoom", function () { scale = 1; apply(); }));

    function fail(msg) {
      $inner.html('<pre class="text-xs text-red-600 whitespace-pre-wrap">' + esc(msg) + "\n\n" + esc(source) + "</pre>");
    }
    if (typeof mermaid === "undefined" || !source.trim()) { fail("Diagram unavailable"); return; }
    try {
      mermaid.render("mmd-" + (++diagramSeq), source)
        .then(function (res) { $inner.html(res.svg); $inner.find("svg").css({ maxWidth: "none", height: "auto" }); })
        .catch(function (err) { fail("Diagram failed to render: " + (err && err.message)); });
    } catch (err) {
      fail("Diagram failed to render: " + (err && err.message));
    }
  }

  // ---- Tabs ---------------------------------------------------------------

  var TAB_BASE = "px-3 py-2 border-b-2 -mb-px text-sm";
  function setActive(tab, on) {
    tab.$btn.attr("class", TAB_BASE + (on
      ? " border-slate-900 text-slate-900 font-medium"
      : " border-transparent text-slate-500 hover:text-slate-700"));
    tab.$panel.toggleClass("hidden", !on);
  }

  // Build a lazy tab set: buttons into $bar, panels into $body. A panel renders
  // on first activation (so G6 graphs size against a visible container).
  function tabset($bar, $body, tabs) {
    $bar.empty(); $body.empty();
    function activate(tab) {
      tabs.forEach(function (t) { setActive(t, t === tab); });
      if (!tab.rendered) { tab.rendered = true; tab.render(tab.$panel); }
    }
    tabs.forEach(function (tab) {
      tab.$btn = $('<button type="button"></button>').text(tab.label).appendTo($bar);
      tab.$panel = $('<div class="hidden"></div>').appendTo($body);
      tab.rendered = false;
      tab.$btn.on("click", function () { activate(tab); });
    });
    if (tabs.length) activate(tabs[0]);
  }

  // ---- Use cases ----------------------------------------------------------

  var modalGraph = null;

  function renderComponentsDiagram($panel, uc) {
    $panel.empty();
    var comps = uc.components || [];
    if (!comps.length) { $panel.html('<div class="text-sm text-slate-400">No components linked.</div>'); return; }
    var $bar = $('<div class="flex gap-1 mb-2"></div>').appendTo($panel);
    var $c = $('<div class="border border-slate-200 rounded" style="height:60vh"></div>').appendTo($panel);
    var nodes = [{ id: "uc", data: { label: uc.name, fill: "#dbeafe" } }];
    var edges = [];
    comps.forEach(function (c, i) {
      nodes.push({ id: "c" + i, data: { label: c.name, fill: "#f1f5f9" } });
      edges.push({ id: "ce" + i, source: "uc", target: "c" + i });
    });
    if (modalGraph) { try { modalGraph.destroy(); } catch (e) { /* ignore */ } modalGraph = null; }
    modalGraph = flowchart($c[0], nodes, edges, null);
    attachG6Controls($bar, modalGraph);
  }

  function renderFlowChart($panel, uc) {
    $panel.empty();
    var diagrams = uc.diagrams || [];
    if (!diagrams.length) { $panel.html('<div class="text-sm text-slate-400">No flow chart for this use case.</div>'); return; }
    diagrams.forEach(function (dg) { renderDiagram($panel, dg); });
  }

  function openUseCaseModal(uc) {
    $("#uc-modal-title").text(uc.name || "Use case");
    $("#uc-modal-desc").text(uc.description || "").toggle(!!uc.description);
    PI.openModal("#uc-modal");
    // Defer to the next frame so the now-visible modal has laid out and the G6
    // canvas sizes correctly.
    requestAnimationFrame(function () {
      tabset($("#uc-modal-tabs"), $("#uc-modal-body"), [
        { label: "Components Diagram", render: function ($p) { renderComponentsDiagram($p, uc); } },
        { label: "Flow Chart", render: function ($p) { renderFlowChart($p, uc); } },
      ]);
    });
  }

  function renderUseCases($panel, d) {
    var useCases = d.use_cases || [];
    if (!useCases.length) { $panel.html('<div class="text-sm text-slate-400">No use cases.</div>'); return; }
    $panel.html(
      '<div class="bg-white rounded-lg shadow border border-slate-200 p-2">' +
        '<div class="flex items-center justify-between gap-2 px-2 pt-1">' +
          '<div class="text-xs text-slate-400">Click a use case to open its diagrams.</div>' +
          '<div class="flex gap-1" id="uc-flow-ctrls"></div></div>' +
        '<div id="uc-flow" style="height:60vh"></div></div>'
    );
    var nodes = [{ id: "app", data: { label: d.name, fill: "#dbeafe" } }];
    var edges = [];
    useCases.forEach(function (uc, i) {
      nodes.push({ id: "uc" + i, data: { label: uc.name, fill: "#fef3c7", ucIndex: i } });
      edges.push({ id: "ue" + i, source: "app", target: "uc" + i });
    });
    var g = flowchart(document.getElementById("uc-flow"), nodes, edges, function (data) {
      if (data.ucIndex != null) openUseCaseModal(useCases[data.ucIndex]);
    });
    attachG6Controls($("#uc-flow-ctrls"), g);
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

  // ---- Page assembly ------------------------------------------------------

  function render(d) {
    $("#detail-title").text(d.name || "Application");
    $("#crumb-current").text(d.name || "Application");

    var tabs = [];
    tabs.push({ label: "Overview", render: function ($p) {
      $p.html(
        '<div class="grid grid-cols-1 lg:grid-cols-3 gap-4">' +
          '<div class="lg:col-span-2 bg-white rounded-lg shadow border border-slate-200 p-2">' +
            '<div class="flex items-center justify-end gap-1 mb-1" id="ov-ctrls"></div>' +
            '<div id="ov-graph" style="height:480px"></div></div>' +
          '<div class="bg-white rounded-lg shadow border border-slate-200 p-4">' +
            '<div id="ov-props"></div><div id="ov-langs" class="mt-4"></div></div>' +
        "</div>"
      );
      $("#ov-props").html(propsHtml(d));
      $("#ov-langs").html(languagesHtml(d));
      attachG6Controls($("#ov-ctrls"), overviewGraph(document.getElementById("ov-graph"), d));
    } });

    tabs.push({ label: "Use cases", render: function ($p) { renderUseCases($p, d); } });

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

    var $root = $("#app-detail").empty();
    var $bar = $('<div class="border-b border-slate-200 mb-4 flex gap-1 flex-wrap"></div>').appendTo($root);
    var $body = $("<div></div>").appendTo($root);
    tabset($bar, $body, tabs);
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
