/**
 * A command's refusal, thrown by a computed answer.
 *
 * The runtime rejects with the refusal's text and nothing else, so a fixture
 * does the same: a screen built against fixtures meets the message it will
 * meet for real.
 */
export class Refusal {
  constructor(readonly message: string) {}
}
