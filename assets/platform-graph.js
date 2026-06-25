// Platform connection graph rendered with cytoscape (served locally).
(function ($) {
  "use strict";

  var cy = null;
  var full = { nodes: [], edges: [] };

  var COLORS = { application: "#2563eb", infrastructure: "#059669", external: "#9ca3af" };

  function elements(showInfra) {
    var nodes = full.nodes.filter(function (n) {
      return showInfra || n.data.kind !== "infrastructure";
    });
    var visible = {};
    nodes.forEach(function (n) { visible[n.data.id] = true; });
    var edges = full.edges.filter(function (e) {
      return visible[e.data.source] && visible[e.data.target];
    });
    return nodes.concat(edges);
  }

  function draw(showInfra) {
    cy = cytoscape({
      container: document.getElementById("cy"),
      elements: elements(showInfra),
      style: [
        {
          selector: "node",
          style: {
            "background-color": function (n) { return COLORS[n.data("kind")] || "#64748b"; },
            label: "data(label)",
            "font-size": "9px",
            color: "#0f172a",
            "text-valign": "bottom",
            width: 16,
            height: 16,
          },
        },
        {
          selector: "edge",
          style: {
            width: 1,
            "line-color": "#cbd5e1",
            "target-arrow-color": "#cbd5e1",
            "target-arrow-shape": "triangle",
            "curve-style": "bezier",
            label: "data(kind)",
            "font-size": "7px",
            color: "#94a3b8",
          },
        },
      ],
      layout: { name: "cose", animate: false },
    });

    // Drill into an application node.
    cy.on("tap", "node", function (evt) {
      var d = evt.target.data();
      if (d.kind === "application") {
        window.location.href = "/platform/applications/" + d.id;
      }
    });
  }

  function load() {
    $.getJSON("/api/platform/graph").done(function (g) {
      full = { nodes: g.nodes, edges: g.edges };
      if (g.truncated) {
        $("#graph-notice").text("Showing a subset of " + g.total_applications + " applications.");
      }
      draw($("#show-infra").is(":checked"));
    });
  }

  $(function () {
    load();
    $("#show-infra").on("change", function () { draw($(this).is(":checked")); });
  });
})(jQuery);
