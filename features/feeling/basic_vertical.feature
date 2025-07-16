Feature: Layka receives a sensation and logs an Instant

  Scenario: A sensor sends a text sensation
    Given a running psycheOS instance named Layka
    And the file /run/quick.sock is available
    When a client sends the following input:
      """
      /chat
      I feel lonely
      ---
      """
    Then the system should append a new Sensation to soul/memory/sensation.jsonl
    And the system should distill an Instant with how = "The interlocutor feels lonely"
    And the Instant should also be appended to soul/memory/sensation.jsonl
    And a Situation summarizing the Instant should be appended to soul/memory/situation.jsonl
