// The two behaviours that were inline `hx-on` attributes.
//
// They moved out here so the Content-Security-Policy can be `script-src 'self'`:
// an inline handler needs `unsafe-inline`, and `unsafe-inline` is most of what a CSP
// exists to forbid. Neither is load-bearing — without JavaScript the forms still
// post, they just keep what was typed.
(function () {
  "use strict";

  // An add form clears itself once its request lands, so the next item can be typed
  // straight away. The form is deliberately outside the swapped region, so it — and
  // the cursor in it — survives.
  document.body.addEventListener("htmx:afterRequest", function (event) {
    var form = event.target;
    if (form instanceof HTMLFormElement && form.classList.contains("add")) {
      form.reset();
    }
  });

  // A list edited on another device updates this one.
  //
  // The server sends a nudge, never the rows: this page re-reads through the ordinary
  // fragment route, so a screen refreshed by an event and one refreshed by its own
  // edit are rendered by the same code and cannot come out different.
  var live = document.getElementById("live");
  if (live && window.EventSource) {
    var pending = false;
    var connected = false;

    // Not while somebody is typing in the list. A swap replaces the field under the
    // cursor, which loses the edit and the caret with it -- so the refresh waits for
    // the focus to leave, which is also when it stops being disruptive.
    //
    // Typing, specifically. Any focused element used to count, and a button keeps
    // the focus after it is clicked: ticking one item off parked the focus inside
    // #items and deferred every refresh from then on, so a page stopped updating as
    // soon as it was used. What needs protecting is unsaved text, and only a field
    // holds any.
    function busy() {
      var focused = document.activeElement;
      if (!focused || !focused.closest("#items")) return false;

      var tag = focused.tagName;
      return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
    }

    function refresh() {
      if (busy()) {
        pending = true;
        return;
      }
      pending = false;
      htmx.ajax("GET", live.dataset.items, { target: "#items", swap: "outerHTML" });
    }

    var source = new EventSource(live.dataset.events);
    source.addEventListener("changed", refresh);

    // EventSource reconnects on its own, and anything that changed while it was down
    // was never delivered. The first open is skipped: the page has just been read.
    source.addEventListener("open", function () {
      if (connected) refresh();
      connected = true;
    });

    document.body.addEventListener("focusout", function () {
      // After the focus has actually moved, not while it is moving.
      window.setTimeout(function () {
        if (pending && !busy()) refresh();
      }, 0);
    });
  }

  // A share link carries its token after the `#`, where no server ever sees it. Only
  // the browser can read it back, so this hands it to /join in a form post and takes
  // it out of the address bar in the same breath -- a fragment otherwise stays there,
  // gets bookmarked, and is read over somebody's shoulder.
  var joining = document.getElementById("joining");
  if (joining) {
    var token = window.location.hash.replace(/^#/, "");
    var form = joining.parentNode.querySelector("form");
    if (token && form) {
      history.replaceState(null, "", window.location.pathname);
      var field = document.createElement("input");
      field.type = "hidden";
      field.name = "token";
      field.value = decodeURIComponent(token);
      form.appendChild(field);
      form.submit();
    } else if (!token) {
      joining.textContent = "That link is missing its invitation. Ask for another.";
    }
  }

  // Cancel is a <label> pointing at the panel switch: the browser closes the editor.
  // This only drops the unsaved typing, so that reopening shows what is stored.
  document.body.addEventListener("click", function (event) {
    var cancel = event.target.closest(".cancel");
    if (!cancel) return;
    var form = cancel.closest("form");
    if (form) form.reset();
  });
})();
