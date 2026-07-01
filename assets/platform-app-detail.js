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
    return $('<button type="button" class="btn btn-secondary btn-sm"></button>')
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
          size: [180, 48], radius: 6,
          fill: function (n) { return n.data.fill || "#eff6ff"; },
          stroke: "#94a3b8", lineWidth: 1,
          labelText: function (n) { return n.data.label; },
          labelPlacement: "center", labelFontSize: 10, labelFill: "#0f172a",
          // Wrap/truncate long labels so text stays inside the rectangle.
          labelWordWrap: true, labelMaxWidth: 160, labelMaxLines: 2, labelTextOverflow: "ellipsis",
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
    var $view = $('<div class="overflow-auto border border-slate-200 rounded bg-white" style="height:72vh; text-align:center"></div>').appendTo($wrap);
    var $inner = $('<div style="transform-origin:top center; display:inline-block; text-align:left; padding:8px;"></div>').appendTo($view);
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
        .then(function (res) { $inner.html(res.svg); sizeSvgNaturally($inner.find("svg")); })
        .catch(function (err) { fail("Diagram failed to render: " + (err && err.message)); });
    } catch (err) {
      fail("Diagram failed to render: " + (err && err.message));
    }
  }

  // Mermaid renders sequence/flow SVGs with `width:100%`, which collapses inside
  // an inline-block wrapper (the diagram looks tiny). Pin the SVG to its natural
  // viewBox dimensions so it renders full-size; the zoom buttons take it from there.
  function sizeSvgNaturally($svg) {
    if (!$svg.length) return;
    var vb = ($svg.attr("viewBox") || "").split(/[\s,]+/).map(parseFloat);
    if (vb.length === 4 && vb[2] > 0 && vb[3] > 0) {
      $svg.attr({ width: vb[2], height: vb[3] })
        .css({ maxWidth: "none", width: vb[2] + "px", height: vb[3] + "px" });
    } else {
      $svg.css({ maxWidth: "none", height: "auto" });
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

  // A diagram is a sequence diagram when tagged as such or when its mermaid
  // source is a sequenceDiagram; everything else is treated as a component view.
  function isSequence(d) {
    return (d.kind || "").toLowerCase() === "sequence" || /^\s*sequenceDiagram/i.test(d.content || "");
  }

  function renderDiagramList($panel, diagrams, emptyMsg) {
    $panel.empty();
    if (!diagrams.length) { $panel.html('<div class="text-sm text-slate-400">' + esc(emptyMsg) + "</div>"); return; }
    diagrams.forEach(function (dg) { renderDiagram($panel, dg); });
  }

  function openUseCaseModal(uc) {
    $("#uc-modal-title").text(uc.name || "Use case");
    $("#uc-modal-desc").text(uc.description || "").toggle(!!uc.description);
    var diagrams = uc.diagrams || [];
    var sequence = diagrams.filter(isSequence);
    var component = diagrams.filter(function (d) { return !isSequence(d); });
    PI.openModal("#uc-modal");
    // Defer to the next frame so the now-visible modal has laid out before the
    // first diagram renders.
    requestAnimationFrame(function () {
      var $tabs = $("#uc-modal-tabs");
      tabset($tabs, $("#uc-modal-body"), [
        { label: "Sequence diagram", render: function ($p) { renderDiagramList($p, sequence, "No sequence diagram for this use case."); } },
        { label: "Component diagram", render: function ($p) { renderDiagramList($p, component, "No component diagram for this use case."); } },
      ]);
      // Right-aligned LLM Hints button for this specific use case.
      if (window.PIHints) {
        $('<div class="ml-auto self-center"></div>')
          .append(PIHints.button({ entityType: "use_case", key: uc.name }))
          .appendTo($tabs);
      }
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

  // ---- Components (with a per-row "Details" file viewer) ------------------

  function renderComponents($panel, d) {
    $panel.empty();
    var comps = d.components || [];
    hintBar("component", comps.map(function (c) { return c.name; })).appendTo($panel);
    var $card = $('<div class="bg-white rounded-lg shadow border border-slate-200 p-3 overflow-auto"></div>').appendTo($panel);
    var $tbl = $('<table class="w-full text-sm"></table>').appendTo($card);
    $tbl.append('<thead><tr class="text-left text-slate-500 border-b">' +
      '<th class="py-2 pr-3">Name</th><th class="py-2 pr-3">Kind</th>' +
      '<th class="py-2 pr-3">Description</th><th class="py-2 pr-3">Files</th><th class="py-2"></th></tr></thead>');
    var $tb = $("<tbody></tbody>").appendTo($tbl);
    comps.forEach(function (c) {
      var $tr = $('<tr class="border-b last:border-0 align-top"></tr>');
      $tr.append('<td class="py-2 pr-3">' + esc(c.name) + "</td>");
      $tr.append('<td class="py-2 pr-3">' + (c.kind ? PI.badgeFor(c.kind) : "—") + "</td>");
      $tr.append('<td class="py-2 pr-3">' + esc(c.description || "—") + "</td>");
      $tr.append('<td class="py-2 pr-3 text-slate-500">' + (c.files || []).length + "</td>");
      var $td = $('<td class="py-2"></td>');
      var $btn = $('<button type="button" class="btn btn-primary btn-sm">Details</button>');
      $btn.on("click", function () { if (window.PIFiles) PIFiles.openFiles({ title: c.name, files: c.files || [] }); });
      $tr.append($td.append($btn));
      $tb.append($tr);
    });
  }

  // ---- Interactions (outbound calls; "Details" opens the implementing files) --

  function renderInteractions($panel, d) {
    $panel.empty();
    var deps = d.dependencies || [];
    // Index components so each interaction can resolve to its implementing files.
    var byId = {}, byName = {};
    (d.components || []).forEach(function (c) {
      if (c.id != null) byId[c.id] = c;
      if (c.name != null) byName[c.name] = c;
    });
    var $card = $('<div class="bg-white rounded-lg shadow border border-slate-200 p-3 overflow-auto"></div>').appendTo($panel);
    var $tbl = $('<table class="w-full text-sm"></table>').appendTo($card);
    $tbl.append('<thead><tr class="text-left text-slate-500 border-b">' +
      '<th class="py-2 pr-3">Target</th><th class="py-2 pr-3">Type</th>' +
      '<th class="py-2 pr-3">Description</th><th class="py-2 pr-3">Component</th>' +
      '<th class="py-2 pr-3">Files</th><th class="py-2"></th></tr></thead>');
    var $tb = $("<tbody></tbody>").appendTo($tbl);
    deps.forEach(function (dep) {
      var comp = (dep.component_id != null && byId[dep.component_id]) ||
        (dep.component_name != null && byName[dep.component_name]) || null;
      var files = (comp && comp.files) || [];
      var $tr = $('<tr class="border-b last:border-0 align-top"></tr>');
      var target = dep.target_app_id
        ? '<a class="text-blue-600 hover:underline" href="/platform/applications/' +
            esc(dep.target_app_id) + '">' + esc(dep.target_name) + "</a>"
        : esc(dep.target_name);
      $tr.append('<td class="py-2 pr-3">' + target + "</td>");
      $tr.append('<td class="py-2 pr-3">' + (dep.kind ? PI.badgeFor(dep.kind) : "—") + "</td>");
      $tr.append('<td class="py-2 pr-3">' + esc(dep.description || "—") + "</td>");
      $tr.append('<td class="py-2 pr-3">' + esc(dep.component_name || "—") + "</td>");
      $tr.append('<td class="py-2 pr-3 text-slate-500">' + files.length + "</td>");
      var $td = $('<td class="py-2"></td>');
      if (files.length) {
        var $btn = $('<button type="button" class="btn btn-primary btn-sm">Details</button>');
        $btn.on("click", function () {
          if (window.PIFiles) PIFiles.openFiles({ title: dep.target_name + " — " + (dep.kind || "call"), files: files });
        });
        $td.append($btn);
      } else {
        $td.append('<span class="text-slate-400 text-xs">—</span>');
      }
      $tr.append($td);
      $tb.append($tr);
    });
  }

  // ---- Tables -------------------------------------------------------------

  function entityLink(entity) {
    return function (row) { return row.id ? "/platform/" + entity + "/" + row.id : null; };
  }
  function permsSummary(perms) {
    if (!perms || typeof perms !== "object") return "";
    return Object.keys(perms).filter(function (k) { return perms[k] === true; }).join(", ");
  }

  // A right-aligned "LLM Hints" bar scoped to an entity type (+ entity names).
  function hintBar(entityType, keys) {
    var $bar = $('<div class="flex justify-end mb-2"></div>');
    if (window.PIHints) $bar.append(PIHints.button({ entityType: entityType, keys: keys || [] }));
    return $bar;
  }

  // A tab whose body is a single PI.localTable; an optional `hintType` adds a
  // per-section LLM-hints bar (with the rows' names as selectable scopes).
  function tableRender(opts) {
    return function ($panel) {
      $panel.empty();
      if (opts.hintType) {
        var keys = (opts.rows || []).map(function (r) { return r.name; }).filter(Boolean);
        $panel.append(hintBar(opts.hintType, keys));
      }
      opts.mount = $('<div></div>').appendTo($panel);
      PI.localTable(opts);
    };
  }

  // ---- Page assembly ------------------------------------------------------

  // Page-header actions, shown when the app has a configured repository:
  // "Ask a Question" (opens the Q&A modal) and "Sync" (scoped sync run).
  function renderSyncButton(d) {
    var $actions = $("#app-actions").empty();
    // Always offer a Refresh that re-fetches the detail and re-renders the tabs.
    $actions.append(PI.refreshButton(loadDetail));
    if (!d.repository_id) return;
    var $ask = $('<button type="button" class="btn btn-primary btn-sm">Ask a Question</button>');
    $ask.on("click", function () { if (window.PIAsk) PIAsk.open(); });
    $actions.append($ask);
    var $btn = $('<button type="button" class="btn btn-success btn-sm">Sync</button>');
    var $note = $('<span class="text-xs text-slate-500"></span>');
    $actions.append($btn).append($note);
    $btn.on("click", function () {
      PI.confirm(
        "Sync this repository now? It clones/updates the repository and re-runs analysis.",
        function () {
          $btn.prop("disabled", true);
          $note.text("Scheduling…");
          $.ajax({ url: "/api/platform/applications/" + d.id + "/sync", method: "POST" })
            .done(function (r) {
              $note.html('Sync scheduled · <a class="text-blue-600 hover:underline" href="/jobs/executions/' +
                r.execution_id + '">view job</a>');
              $btn.prop("disabled", false);
            })
            .fail(function (xhr) {
              var err = xhr.responseJSON && xhr.responseJSON.error;
              $note.text("Error: " + ((err && err.message) || "could not schedule sync"));
              $btn.prop("disabled", false);
            });
        }
      );
    });
  }

  function render(d) {
    $("#detail-title").text(d.name || "Application");
    $("#crumb-current").text(d.name || "Application");
    renderSyncButton(d);

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
      hintBar("application").appendTo($p);
    } });

    tabs.push({ label: "Use cases", render: function ($p) {
      hintBar("use_case", (d.use_cases || []).map(function (u) { return u.name; })).appendTo($p);
      var $list = $("<div></div>").appendTo($p);
      renderUseCases($list, d);
    } });

    function linkedTab(key, label) {
      if (!(d[key] || []).length) return;
      tabs.push({ label: label, render: tableRender({
        title: label, rows: d[key],
        columns: [["name", "Name"], ["kind", "Kind"], ["version", "Version"], ["usage", "Usage"]],
        filterKey: "kind", link: entityLink(key), hintType: key.replace(/-/g, "_"),
      }) });
    }
    linkedTab("services", "Services");
    linkedTab("cloud-providers", "Cloud providers");
    linkedTab("platforms", "Platforms");

    if ((d.libraries || []).length) {
      tabs.push({ label: "Libraries", render: tableRender({
        title: "Libraries", rows: d.libraries,
        columns: [["name", "Name"], ["ecosystem", "Ecosystem"], ["version", "Version"], ["scope", "Scope"]],
        filterKey: "ecosystem", link: entityLink("libraries"), hintType: "library",
      }) });
    }
    linkedTab("tools", "Tools");
    linkedTab("external", "External");

    if ((d.components || []).length) {
      tabs.push({ label: "Components", render: function ($p) { renderComponents($p, d); } });
    }

    if ((d.dependencies || []).length) {
      tabs.push({ label: "Interactions", render: function ($p) { renderInteractions($p, d); } });
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
        filterKeys: ["kind", "component"], hintType: "observability_signal",
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

    tabs.push({ label: "Codebase Map", render: function ($p) {
      $p.html('<div class="bg-white rounded-lg shadow border border-slate-200 p-2">' +
        '<div class="flex items-center justify-end gap-1 mb-1" id="cm-ctrls"></div>' +
        '<div id="cm-graph" style="height:520px"></div>' +
        '<div id="cm-note" class="text-xs text-amber-600 mt-1"></div></div>');
      $.ajax({ url: "/api/platform/applications/" + d.id + "/codebase-map", dataType: "json", global: false })
        .done(function (data) {
          if (typeof G6 === "undefined") { $("#cm-graph").html('<div class="text-sm text-slate-400 p-3">Graph library unavailable.</div>'); return; }
          if (data.truncated) $("#cm-note").text("Map truncated for a large repository.");
          var edges = (data.edges || []).map(function (e, i) { return { id: "cm" + i, source: e.source, target: e.target }; });
          // The codebase map is a strict directory containment tree, so render it
          // as a left-to-right compact tree (flowchart) rather than a force blob.
          var graph = new G6.Graph({
            container: document.getElementById("cm-graph"), autoResize: true, autoFit: "view",
            data: { nodes: data.nodes || [], edges: edges },
            layout: {
              type: "compact-box", direction: "LR",
              getId: function (n) { return n.id; },
              getWidth: function () { return 120; },
              getHeight: function () { return 26; },
              getHGap: function () { return 40; },
              getVGap: function () { return 8; },
            },
            node: { type: "rect", style: {
              size: [120, 26], radius: 4, fill: "#eff6ff", stroke: "#94a3b8", lineWidth: 1,
              labelText: function (n) { return n.data.label; }, labelPlacement: "center",
              labelFontSize: 9, labelFill: "#0f172a",
              labelMaxWidth: 110, labelWordWrap: true, labelMaxLines: 1, labelTextOverflow: "ellipsis",
            } },
            edge: { type: "cubic-horizontal", style: { stroke: "#cbd5e1" } },
            behaviors: ["drag-canvas", "zoom-canvas", "drag-element"],
          });
          graph.render();
          attachG6Controls($("#cm-ctrls"), graph);
        })
        .fail(function (xhr) {
          var err = xhr.responseJSON && xhr.responseJSON.error;
          $("#cm-graph").html('<div class="text-sm text-slate-400 p-3">' +
            ((err && err.message) || "Codebase map unavailable — the repository may not be cloned yet.") + "</div>");
        });
    } });

    tabs.push({ label: "File Explorer", render: function ($p) {
      if (window.PIFiles) { PIFiles.mount($p); }
      else { $p.html('<div class="text-sm text-red-600">File explorer unavailable.</div>'); }
    } });

    tabs.push({ label: "Insights", render: function ($p) {
      var base = "/api/platform/applications/" + d.id + "/metrics";
      $p.html('<div class="bg-white rounded-lg shadow border border-slate-200 p-4">' +
        '<div class="flex items-center justify-between mb-2"><div class="font-semibold">Quality metrics</div>' +
        '<button id="metrics-collect" type="button" class="btn btn-primary btn-sm">Collect</button></div>' +
        '<div id="metrics-note" class="text-xs text-slate-500 mb-1"></div>' +
        '<div id="metrics-body" class="text-sm text-slate-500">Loading…</div></div>');
      // Disable Collect while a collection for this app is already queued/running,
      // so we never enqueue a duplicate (the server enforces this too).
      function setCollecting(on, note) {
        $("#metrics-collect").prop("disabled", on);
        $("#metrics-note").text(on ? (note || "Collecting… refresh in a moment.") : "");
      }
      // Metrics are grouped by category (M33). Labels + a preferred section order;
      // any unknown category falls to the end under its raw key.
      var CATEGORY_LABELS = {
        code_health: "Code health", security: "Security & supply chain",
        architecture: "Architecture", model_coverage: "Model coverage",
        delivery: "Delivery", ownership: "Ownership", general: "Other"
      };
      var CATEGORY_ORDER = ["code_health", "security", "architecture", "model_coverage", "delivery", "ownership", "general"];
      function esc(s) { return $("<span>").text(s == null ? "" : s).html(); }
      function metricRow(m) {
        var badge = m.source === "derived"
          ? ' <span class="text-[10px] uppercase tracking-wide text-slate-400">derived</span>' : '';
        return '<tr><td class="pr-4 py-0.5 font-mono">' + esc(m.metric_key) + '</td><td class="py-0.5">' +
          m.value + (m.unit ? ' <span class="text-slate-400">' + esc(m.unit) + '</span>' : '') + badge + "</td></tr>";
      }
      function renderMetrics(ms) {
        var byCat = {};
        ms.forEach(function (m) { var c = m.category || "general"; (byCat[c] = byCat[c] || []).push(m); });
        var cats = CATEGORY_ORDER.filter(function (c) { return byCat[c]; });
        Object.keys(byCat).forEach(function (c) { if (cats.indexOf(c) < 0) { cats.push(c); } });
        return cats.map(function (c) {
          var rows = byCat[c].map(metricRow).join("");
          return '<div class="mb-3"><div class="text-xs font-semibold text-slate-500 uppercase tracking-wide mb-1">' +
            esc(CATEGORY_LABELS[c] || c) + '</div><table class="w-full text-sm">' + rows + "</table></div>";
        }).join("");
      }
      function load() {
        $.ajax({ url: base, dataType: "json", global: false }).done(function (r) {
          setCollecting(!!r.collecting);
          var ms = r.metrics || [];
          if (!ms.length) { $("#metrics-body").html('<div class="text-slate-400">No metrics yet. Click Collect to gather them from CI + the codebase.</div>'); return; }
          $("#metrics-body").html(renderMetrics(ms));
        });
      }
      $("#metrics-collect").on("click", function () {
        setCollecting(true);
        $.ajax({ url: base, method: "POST" })
          .done(function (resp) {
            setCollecting(true, resp && resp.already_running
              ? "Already collecting for this repository — refresh in a moment."
              : "Collecting… refresh in a moment.");
          })
          .fail(function () { setCollecting(false); window.PI.toast("Could not start collection", false); });
      });
      load();
    } });

    tabs.push({ label: "AI Agent", render: function ($p) {
      if (window.PIAgent) { PIAgent.render($p); }
      else { $p.html('<div class="text-sm text-red-600">AI Agent unavailable.</div>'); }
    } });

    var $root = $("#app-detail").empty();
    var $bar = $('<div class="border-b border-slate-200 mb-4 flex gap-1 flex-wrap"></div>').appendTo($root);
    var $body = $("<div></div>").appendTo($root);
    tabset($bar, $body, tabs);
  }

  // Re-fetch the application detail and re-render every tab (the Refresh button
  // in the page header and the initial load both call this).
  function loadDetail() {
    $.getJSON("/api/platform/" + meta.entity + "/" + meta.id)
      .done(function (d) { render(d.detail); })
      .fail(function () { $("#detail-title").text("Not found"); });
  }

  $(function () {
    if (typeof mermaid !== "undefined") {
      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        flowchart: { useMaxWidth: false },
        sequence: { useMaxWidth: false },
      });
    }
    // Friendly property names are best-effort; render regardless of the result.
    $.getJSON("/api/settings/entity-properties")
      .always(function (p) { buildPropMap(p && p.properties); })
      .always(loadDetail);
  });
})(jQuery);
