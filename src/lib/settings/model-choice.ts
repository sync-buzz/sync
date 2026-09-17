/**
 * Which agent this installation works through.
 *
 * The list an agent is chosen from is decided in Rust and never in the
 * window: which agents can be raised is the registry's measurement. A list
 * built here would be a second answer to a question the launch already
 * answers, and it would look right while disagreeing.
 */

import { command } from "@/lib/command";

/** One provider, as the window offers it. */
export interface Provider {
  /** What a choice stores. */
  readonly id: string;
  /** What a person reads. */
  readonly name: string;
  /** Whether this one can be chosen at all. */
  readonly eligible: boolean;
  /**
   * The file its tools would be read from, as a person would recognise it.
   *
   * `null` where Sync does not know one, which is one of the two reasons a
   * provider cannot be chosen.
   */
  readonly configuration: string | null;
  /** Why it cannot be chosen, in a sentence, or `null` when it can. */
  readonly refusal: string | null;
}

export interface ModelChoiceStatus {
  /** Every provider, the unchoosable ones carrying their reason. */
  readonly cloud: readonly Provider[];
  /** Which of them is chosen, or `null` while nobody has picked. */
  readonly cloudChosen: string | null;
}

/** The whole decision, and whether it is working. */
export function loadModelChoice(): Promise<ModelChoiceStatus> {
  return command<ModelChoiceStatus>("model_choice_status", {});
}

/**
 * Write the decision down, and read back what the section should now show.
 */
export function setModelChoice(choice: { cloudAgent: string | null }): Promise<ModelChoiceStatus> {
  return command<ModelChoiceStatus>("model_choice_set", choice);
}
