// The one animated moment on the page: a typed line becoming an item.
// Everything else is static, and this is skipped entirely for anyone who has asked
// their system for less motion.
(function () {
  var typed = document.getElementById("typed");
  var caret = document.getElementById("caret");
  var arriving = document.getElementById("arriving");
  if (!typed || !arriving) return;

  var line = "2 kg apples";
  var still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function land() {
    arriving.classList.remove("is-arriving");
    arriving.classList.add("is-arrived");
    if (caret) caret.style.display = "none";
    var count = document.getElementById("count");
    if (count) count.textContent = "7 items";
  }

  if (still) {
    typed.textContent = line;
    land();
    return;
  }

  var i = 0;
  function tick() {
    typed.textContent = line.slice(0, ++i);
    if (i < line.length) {
      // Uneven, because a person typing is uneven.
      setTimeout(tick, 55 + Math.random() * 65);
    } else {
      setTimeout(land, 420);
    }
  }
  setTimeout(tick, 650);
})();
