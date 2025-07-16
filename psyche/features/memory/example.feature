Feature: Memory storage
  Scenario: store a sensation
    Given a fresh memory store
    When I store a sensation
    Then it should be retrievable
