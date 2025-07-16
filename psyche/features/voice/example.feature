Feature: Voice interaction
  Scenario: speak aloud
    Given the daemon is running
    When I send a phrase
    Then it should be spoken
