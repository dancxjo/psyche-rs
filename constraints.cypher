CREATE CONSTRAINT FOR (s:Sensation) REQUIRE s.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (i:Impression) REQUIRE i.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (n:Intention) REQUIRE n.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (c:Completion) REQUIRE c.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (r:Interruption) REQUIRE r.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (m:MotorCall) REQUIRE m.uuid IS UNIQUE;
CREATE CONSTRAINT FOR (ls:Lifecycle) REQUIRE ls.uuid IS UNIQUE;
