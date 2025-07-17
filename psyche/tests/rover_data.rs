use assert_cmd::Command;
use httpmock::prelude::*;

#[test]
fn download_rover_data_streams_to_socket() {
    // start a mock NASA server
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/mars-photos/api/v1/rovers/perseverance/photos")
            .query_param("sol", "1000")
            .query_param("api_key", "DEMO_KEY");
        then.status(200)
            .header("Content-Type", "application/json")
            .body("{\"photos\":[{\"rover\":{\"name\":\"Perseverance\"},\"earth_date\":\"2023-01-01\",\"img_src\":\"http://example.com/1.jpg\"}]}");
    });

    let output = Command::new("bash")
        .arg("../scripts/download-mars-rover-data.sh")
        .arg("-")
        .env("NASA_API_BASE_URL", server.url(""))
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Perseverance"));

    mock.assert();
}
