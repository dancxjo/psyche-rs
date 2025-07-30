// Constraints for Neo4j memory graph
CREATE CONSTRAINT unique_person_id IF NOT EXISTS FOR (p:Person) REQUIRE p.id IS UNIQUE;
CREATE CONSTRAINT unique_face_id IF NOT EXISTS FOR (f:Face) REQUIRE f.id IS UNIQUE;
