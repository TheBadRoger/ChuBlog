import type { SessionState } from './lib/api';

declare global {
  namespace App {
    interface Locals {
      accountState?: Promise<SessionState>;
    }
  }
}
export {};
