import { defineMiddleware } from 'astro:middleware';
import { readSession } from './lib/session';

export const onRequest = defineMiddleware(async (context, next) => {
  if (context.request.headers.get('accept')?.includes('text/html')) void readSession(context);
  const response = await next();
  response.headers.set('Cache-Control', 'no-store');
  response.headers.set('X-Content-Type-Options', 'nosniff');
  response.headers.set('Referrer-Policy', 'same-origin');
  response.headers.set('Content-Security-Policy', "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'");
  return response;
});
