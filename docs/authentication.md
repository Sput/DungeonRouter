# Supabase authentication

DungeonRouter uses Supabase Auth for email/password sessions. The React client signs in with the official Supabase JavaScript client and stores its refreshable session in browser storage. Every protected API request includes the current access token as `Authorization: Bearer <token>`.

The Rust API does not trust the presence of a browser session. Its authentication middleware validates each Bearer token with the Supabase Auth `/auth/v1/user` endpoint before allowing access to chat, model routes, sources, campaign notes, search, activity, or usage data. Only `GET /api/health` is public.

## Required configuration

Provide the same project URL and anon key to the browser and API:

```dotenv
SUPABASE_URL=https://your-project.supabase.co
SUPABASE_ANON_KEY=your-anon-key
VITE_SUPABASE_URL=https://your-project.supabase.co
VITE_SUPABASE_ANON_KEY=your-anon-key
```

The anon key identifies the project but does not grant administrative access, so it is appropriate for a browser bundle.

Never put the Supabase `service_role`, secret key, database password, or JWT signing secret in a `VITE_` variable. DungeonRouter does not require any of them.

Vite variables are embedded at frontend build time. In Coolify, configure the two `VITE_` variables as build-time values and configure the two server variables on the API container at runtime.

## Supabase project setup

1. Create the project and keep Email authentication enabled.
2. In **Authentication > Users**, create or invite the user who should have access.
3. Disable **Allow new users to sign up** in the Auth general configuration. With signup disabled, only users you create can sign in.
4. Set the four environment variables above.
5. Rebuild the frontend and restart the API.

The application intentionally has no signup or password-reset page. User lifecycle remains an administrator action in the Supabase Dashboard.

## Local development

After adding the variables to `.env`, make the Vite values available to the web process. Vite reads environment files from `apps/web`, so either create `apps/web/.env.local` with the two `VITE_` values or export them in the terminal before running `pnpm dev:web`.

The Rust API reads the non-Vite values from the repository-root `.env` when it starts.

## Security behavior

- Missing and rejected tokens return HTTP 401.
- An unavailable Supabase Auth service returns HTTP 503.
- Provider response bodies and access tokens are not returned in application errors.
- Signing out removes the browser session and returns to the login page.
- A valid Supabase user currently has access to the entire local DungeonRouter dataset; per-user campaign isolation has not been implemented.
