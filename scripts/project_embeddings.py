import argparse
import json
from qdrant_client import QdrantClient
from sklearn.decomposition import PCA
from sklearn.cluster import DBSCAN
import requests


def main():
    parser = argparse.ArgumentParser(description="Project Qdrant embeddings to 3D and store in Neo4j")
    parser.add_argument("--qdrant", default="http://localhost:6333", help="Qdrant base URL")
    parser.add_argument("--neo4j", default="http://localhost:7474", help="Neo4j base URL")
    parser.add_argument("--neo-user", default="neo4j")
    parser.add_argument("--neo-pass", default="password")
    parser.add_argument("--limit", type=int, default=1000)
    parser.add_argument("--eps", type=float, default=0.5)
    parser.add_argument("--min-samples", type=int, default=5)
    parser.add_argument("--out", default="embedding_positions.json")
    args = parser.parse_args()

    client = QdrantClient(url=args.qdrant)
    points, _ = client.scroll(collection_name="impressions", with_payload=True, limit=args.limit)
    if not points:
        print("No embeddings found")
        return
    ids = [p.id for p in points]
    vecs = [p.vector for p in points]

    reducer = PCA(n_components=3)
    coords = reducer.fit_transform(vecs)
    clustering = DBSCAN(eps=args.eps, min_samples=args.min_samples).fit(coords)
    labels = clustering.labels_

    data = []
    for pid, pos, lbl in zip(ids, coords, labels):
        entry = {"uuid": pid, "pos": pos.tolist(), "cluster": int(lbl)}
        data.append(entry)
        query = (
            "MATCH (i:Impression {uuid:$id}) "
            "SET i.embedding3d=$pos, i.cluster=$cluster"
        )
        payload = {
            "statements": [
                {
                    "statement": query,
                    "parameters": {"id": pid, "pos": pos.tolist(), "cluster": int(lbl)},
                }
            ]
        }
        requests.post(
            f"{args.neo4j}/db/neo4j/tx/commit",
            json=payload,
            auth=(args.neo_user, args.neo_pass),
            timeout=10,
        )

    with open(args.out, "w") as f:
        json.dump(data, f)


if __name__ == "__main__":
    main()
