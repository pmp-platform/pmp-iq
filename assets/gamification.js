// Gamification leaderboard + profile (M44).
(function ($) {
  "use strict";

  function profile() {
    $.ajax({ url: "/api/gamification/me", dataType: "json" })
      .done(function (d) {
        var $p = $("#my-profile").empty();
        $p.append($('<div class="text-lg font-semibold">').text("Level " + d.level.level + " · " + d.total_xp + " XP"));
        $p.append($('<div class="text-xs text-slate-500 mb-2">').text(d.level.to_next + " XP to next level"));
        if ((d.skills || []).length) {
          $p.append($('<div class="text-xs font-semibold mt-1">').text("Skills"));
          d.skills.forEach(function (s) { $p.append($('<span class="inline-block text-xs bg-slate-100 rounded px-1.5 py-0.5 mr-1">').text(s.skill + " " + s.points)); });
        }
        if ((d.badges || []).length) {
          $p.append($('<div class="text-xs font-semibold mt-2">').text("Badges"));
          d.badges.forEach(function (b) { $p.append($('<span class="inline-block text-xs bg-amber-100 text-amber-700 rounded px-1.5 py-0.5 mr-1">').text(b.replace(/_/g, " "))); });
        }
        if (!d.total_xp) { $p.append($('<div class="text-xs text-slate-400 mt-2">').text("No XP yet — take some actions!")); }
      })
      .fail(function () { $("#my-profile").text("Could not load profile."); });
  }

  function board() {
    $.ajax({ url: "/api/gamification/leaderboard", dataType: "json" })
      .done(function (d) {
        var $t = $("#leaderboard-rows").empty();
        (d.leaderboard || []).forEach(function (r) {
          $t.append($("<tr class='border-b border-slate-100'>")
            .append($('<td class="p-1.5">').text(r.actor))
            .append($("<td>").text(Math.floor((r.points || 0) / 100) + 1))
            .append($("<td>").text(r.points)));
        });
        if (!(d.leaderboard || []).length) { $t.append("<tr><td colspan='3' class='p-1.5 text-slate-400'>No awards yet.</td></tr>"); }
      });
  }

  $(function () {
    profile();
    board();
    $("#gam-replay").on("click", function () {
      $.ajax({ url: "/api/gamification/replay", method: "POST" }).always(function () { profile(); board(); });
    });
  });
})(jQuery);
