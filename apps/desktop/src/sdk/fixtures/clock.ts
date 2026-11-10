/**
 * Every fixture time is set relative to the moment the fixtures load, to the
 * minute. The screens measure against the real clock, so a fixed date reads as
 * weeks overdue in a schedule, weeks ago on the dashboard, and a live run whose
 * timeline is all waiting; relative times read as the demo state they describe.
 */
export const now = new Date(Math.floor(Date.now() / 60_000) * 60_000);
export const minutesAgo = (n: number) => new Date(now.getTime() - n * 60_000).toISOString();
export const minutesAhead = (n: number) => minutesAgo(-n);
