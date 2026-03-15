var CACHE_NAME = 'missive-v1';
var OFFLINE_URL = '/static/offline.html';

var PRECACHE_URLS = [
  '/static/css/app.css',
  '/static/css/trix.css',
  '/static/js/trix.js',
  '/static/favicon.ico',
  '/static/images/icon-192x192.png',
  '/static/images/icon-512x512.png',
  OFFLINE_URL
];

self.addEventListener('install', function(event) {
  event.waitUntil(
    caches.open(CACHE_NAME).then(function(cache) {
      return cache.addAll(PRECACHE_URLS);
    }).then(function() {
      return self.skipWaiting();
    })
  );
});

self.addEventListener('activate', function(event) {
  event.waitUntil(
    caches.keys().then(function(names) {
      return Promise.all(
        names.filter(function(name) { return name !== CACHE_NAME; })
             .map(function(name) { return caches.delete(name); })
      );
    }).then(function() {
      return self.clients.claim();
    })
  );
});

self.addEventListener('fetch', function(event) {
  var request = event.request;

  // Only handle GET requests
  if (request.method !== 'GET') return;

  // Skip SSE and API requests
  var url = new URL(request.url);
  if (url.pathname.startsWith('/api/')) return;

  // Static assets: cache-first
  if (url.pathname.startsWith('/static/')) {
    event.respondWith(
      caches.match(request).then(function(cached) {
        if (cached) return cached;
        return fetch(request).then(function(response) {
          if (response.ok) {
            var clone = response.clone();
            caches.open(CACHE_NAME).then(function(cache) {
              cache.put(request, clone);
            });
          }
          return response;
        });
      })
    );
    return;
  }

  // HTML pages: network-first, fall back to offline page
  if (request.headers.get('Accept') && request.headers.get('Accept').indexOf('text/html') !== -1) {
    event.respondWith(
      fetch(request).catch(function() {
        return caches.match(OFFLINE_URL);
      })
    );
    return;
  }
});
