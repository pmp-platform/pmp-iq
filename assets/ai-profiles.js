// Settings page: AI agent profiles table + CRUD/test API.
(function ($) {
  "use strict";

  var MODELS = [
    "claude-opus-4-8",
    "claude-fable-5",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-haiku-4-5",
  ];
  var EFFORTS = ["low", "medium", "high", "xhigh", "max"];

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
          "<td>" + window.PI.badge(p.enabled ? "Enabled" : "Disabled", p.enabled ? "success" : "danger") + "</td>" +
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

  function fillSelect(sel, values, leadingLabel) {
    var $sel = $(sel).empty();
    if (leadingLabel) $sel.append($("<option>").val("").text(leadingLabel));
    values.forEach(function (v) { $sel.append($("<option>").val(v).text(v)); });
  }

  // Show only the fields relevant to the chosen provider and tailor the API-key help.
  function updateProviderUI() {
    var provider = $("#profile-provider").val();
    var isAnthropic = provider === "anthropic";
    $("#effort-field, #max-tokens-field, #base-url-field").toggleClass("hidden", !isAnthropic);
    $("#binary-path-field, #extra-args-field").toggleClass("hidden", isAnthropic);
    if (isAnthropic) {
      $("#profile-api-key").attr("placeholder", "API key (required, stored encrypted)");
      $("#api-key-help").text("Your Anthropic API key. Stored encrypted at rest.");
    } else {
      $("#profile-api-key").attr("placeholder", "API key (optional)");
      $("#api-key-help").text(
        "Optional. If left blank, the Claude CLI uses its own configured authentication " +
        "(e.g. `claude login`, or ANTHROPIC_API_KEY in its environment). Set a key only to " +
        "override that for this profile."
      );
    }
  }

  // Build the provider-specific config object from the form fields.
  function buildConfig(provider) {
    var config = {};
    var model = $("#profile-model").val();
    if (model) config.model = model;
    if (provider === "anthropic") {
      var effort = $("#profile-effort").val();
      if (effort) config.effort = effort;
      var maxTokens = $("#profile-max-tokens").val();
      if (maxTokens) config.max_tokens = parseInt(maxTokens, 10);
      var baseUrl = $("#profile-base-url").val().trim();
      if (baseUrl) config.base_url = baseUrl;
    } else {
      var binaryPath = $("#profile-binary-path").val().trim();
      if (binaryPath) config.binary_path = binaryPath;
      var extraArgs = $("#profile-extra-args").val().trim();
      if (extraArgs) config.extra_args = extraArgs.split(/\s+/);
    }
    return config;
  }

  $(function () {
    load();
    fillSelect("#profile-model", MODELS, "Default model");
    fillSelect("#profile-effort", EFFORTS, "Default effort");
    updateProviderUI();
    $("#profile-provider").on("change", updateProviderUI);

    $("#profile-form").on("submit", function (e) {
      e.preventDefault();
      var provider = $("#profile-provider").val();
      var data = {
        name: $(this).find("[name=name]").val(),
        provider_type: provider,
        config: buildConfig(provider),
        enabled: true,
      };
      var apiKey = $("#profile-api-key").val();
      if (apiKey) data.api_key = apiKey;
      $.ajax({
        url: "/api/settings/ai-profiles",
        method: "POST",
        contentType: "application/json",
        data: JSON.stringify(data),
      }).done(function () {
        flash("", true);
        load();
        $("#profile-form")[0].reset();
        updateProviderUI();
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
