// Redirects www.herdrui.dev to the apex domain and serves everything else from
// the static assets.
export default {
  fetch(request, env) {
    const url = new URL(request.url);
    if (url.hostname === 'www.herdrui.dev') {
      url.hostname = 'herdrui.dev';
      return Response.redirect(url.toString(), 301);
    }
    return env.ASSETS.fetch(request);
  },
};
