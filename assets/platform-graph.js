// Platform connection graph rendered with AntV G6 v5 (served locally).
(function ($) {
  "use strict";

  var graph = null;
  var full = { nodes: [], edges: [] };

  // [kind, label, color]. "application" is always shown (no toggle); every other
  // kind matches a linked-entity registry name plus the synthetic "external".
  var KINDS = [
    ["application", "Applications", "#2563eb"],
    ["infrastructure", "Infrastructure", "#059669"],
    ["tools", "Tools", "#d97706"],
    ["cloud-providers", "Cloud providers", "#7c3aed"],
    ["services", "Services", "#db2777"],
    ["platforms", "Platforms", "#0891b2"],
    ["external", "External", "#9ca3af"],
  ];
  var COLORS = {};
  KINDS.forEach(function (k) { COLORS[k[0]] = k[2]; });

  // The API returns cytoscape-style data ({data:{id,label,kind,href}}); map to G6.
  function transform(g) {
    return {
      nodes: (g.nodes || []).map(function (n) {
        return { id: n.data.id, data: { label: n.data.label, kind: n.data.kind, href: n.data.href } };
      }),
      edges: (g.edges || []).map(function (e, i) {
        var d = e.data || {};
        return { id: d.id || "e" + i, source: d.source, target: d.target, data: { kind: d.kind || "" } };
      }),
    };
  }

  // Kinds whose toggle is currently unchecked.
  function hiddenKinds() {
    var off = {};
    $("#graph-toggles input[type=checkbox]").each(function () {
      if (!$(this).is(":checked")) off[$(this).data("kind")] = true;
    });
    return off;
  }

  // Current dataset filtered by the kind toggles.
  function elements() {
    var off = hiddenKinds();
    var nodes = full.nodes.filter(function (n) { return !off[n.data.kind]; });
    var visible = {};
    nodes.forEach(function (n) { visible[n.id] = true; });
    var edges = full.edges.filter(function (e) { return visible[e.source] && visible[e.target]; });
    return { nodes: nodes, edges: edges };
  }

  function create(data) {
    graph = new G6.Graph({
      container: "cy",
      autoResize: true,
      autoFit: "view",
      data: data,
      // Spread nodes out: longer links, stronger repulsion, collision padding.
      layout: {
        type: "d3-force",
        link: { distance: 110 },
        manyBody: { strength: -300 },
        collide: { radius: 28 },
      },
      node: {
        style: {
          size: 14,
          fill: function (d) { return COLORS[d.data.kind] || "#64748b"; },
          stroke: "#ffffff",
          lineWidth: 1,
          labelText: function (d) { return d.data.label; },
          labelPlacement: "bottom",
          labelFontSize: 8,
          labelFill: "#0f172a",
        },
      },
      edge: {
        style: {
          stroke: "#cbd5e1",
          endArrow: true,
          labelText: function (d) { return d.data.kind; },
          labelFontSize: 7,
          labelFill: "#94a3b8",
        },
      },
      behaviors: ["zoom-canvas", "drag-canvas", "drag-element"],
    });

    // Drill into a node when it carries a navigation target.
    graph.on("node:click", function (evt) {
      var id = evt.target && evt.target.id;
      if (!id) return;
      var node = graph.getNodeData(id);
      if (node && node.data && node.data.href) {
        window.location.href = node.data.href;
      }
    });

    graph.render();
  }

  function redraw() {
    var data = elements();
    if (!graph) {
      create(data);
    } else {
      graph.setData(data);
      graph.render();
    }
  }

  // Build the legend and per-kind visibility toggles from KINDS.
  function buildControls() {
    var $toggles = $("#graph-toggles").empty();
    var $legend = $("#graph-legend").empty();
    KINDS.forEach(function (k) {
      $legend.append('<span><span class="inline-block w-3 h-3 rounded-full" style="background:' +
        k[2] + '"></span> ' + k[1] + "</span>");
      if (k[0] === "application") return;
      // Start with applications only; every other kind is opt-in via its toggle,
      // keeping the initial view focused on the application landscape.
      $toggles.append('<label class="flex items-center gap-1"><input type="checkbox" data-kind="' +
        k[0] + '" /> ' + k[1] + "</label>");
    });
    $toggles.on("change", "input", redraw);
  }

  function load() {
    $.getJSON("/api/platform/graph").done(function (g) {
      full = transform(g);
      if (g.truncated) {
        $("#graph-notice").text("Showing a subset of " + g.total_applications + " applications.");
      }
      redraw();
    });
  }

  $(function () {
    buildControls();
    load();
  });
})(jQuery);
