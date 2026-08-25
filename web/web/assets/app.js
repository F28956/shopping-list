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

  // Cancel is a <label> pointing at the panel switch: the browser closes the editor.
  // This only drops the unsaved typing, so that reopening shows what is stored.
  document.body.addEventListener("click", function (event) {
    var cancel = event.target.closest(".cancel");
    if (!cancel) return;
    var form = cancel.closest("form");
    if (form) form.reset();
  });
})();
