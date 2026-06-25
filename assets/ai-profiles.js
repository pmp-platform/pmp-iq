// Settings page: AI agent profiles table + CRUD/test API.
(function ($) {
  "use strict";

  function render(profiles) {
    var $body = $("#profiles-table tbody").empty();
    if (!profiles.length) {
      $body.append('<tr><td class="p-3 text-slate-400" colspan="4">No profiles yet.</td></tr>');
      return;
    }
    profiles.forEach(function (p) {
      var $row = $(
        '<tr class="border-b">' +
          '<td class="p-3">' + $("<div>").text(p.name).html() + "</td>" +
          "<td>" + p.provider_type + "</td>" +
          "<td>" + (p.enabled ? "yes" : "no") + "</td>" +
          '<td class="text-right pr-3 whitespace-nowrap">' +
            window.PI.actionButton("Test", { "data-act": "test" }) +
            window.PI.actionButton("Validate", { "data-act": "validate" }) +
            window.PI.actionButton("Delete", { "data-act": "del" }, "danger") +
          "</td>" +
        "</tr>"
      );
      $row.find("button").data("id", p.id);
      $body.append($row);
    });
  }

  function load() {
    $.getJSON("/api/settings/ai-profiles").done(function (d) { render(d.profiles); });
  }

  function flash(msg, ok) {
    $("#profile-msg").text(msg).css("color", ok ? "#15803d" : "#b91c1c");
  }

  $(function () {
    load();

    $("#profile-form").on("submit", function (e) {
      e.preventDefault();
      var data = {};
      $.each($(this).serializeArray(), function (_, f) { data[f.name] = f.value; });
      data.enabled = true;
      if (data.config) {
        try { data.config = JSON.parse(data.config); }
        catch (err) { flash("Invalid config JSON", false); return; }
      } else {
        delete data.config;
      }
      $.ajax({
        url: "/api/settings/ai-profiles",
        method: "POST",
        contentType: "application/json",
        data: JSON.stringify(data),
      }).done(function () {
        flash("", true);
        load();
        $("#profile-form")[0].reset();
        window.PI.closeModal("#profile-modal");
      }).fail(function (x) { flash("Error: " + x.responseText, false); });
    });

    $("#profiles-table").on("click", "button", function () {
      var id = $(this).data("id");
      var act = $(this).data("act");
      if (act === "del") {
        window.PI.confirm("Delete this AI profile?", function () {
          $.ajax({ url: "/api/settings/ai-profiles/" + id, method: "DELETE" }).done(load);
        });
      } else if (act === "validate") {
        $.ajax({ url: "/api/settings/ai-profiles/" + id + "/validate", method: "POST" })
          .done(function () { window.PI.toast("Profile OK", true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      } else if (act === "test") {
        var prompt = window.prompt("Test prompt:", "Say hello in one word.");
        if (!prompt) return;
        $.ajax({
          url: "/api/settings/ai-profiles/" + id + "/test",
          method: "POST",
          contentType: "application/json",
          data: JSON.stringify({ prompt: prompt }),
        }).done(function (d) { window.PI.toast("Response: " + d.response.text, true); })
          .fail(function (x) { window.PI.toast("Failed: " + x.responseText, false); });
      }
    });
  });
})(jQuery);
