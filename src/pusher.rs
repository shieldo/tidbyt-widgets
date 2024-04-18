pub mod pusher {
    use anyhow::Result;
    use reqwest::header::USER_AGENT;
    use serde::Serialize;

    use base64::{engine::general_purpose, Engine as _};

    #[derive(Serialize, Debug)]
    struct TidbytPayload {
        #[serde(rename = "deviceID")]
        device_id: String,
        #[serde(rename = "installationID")]
        installation_id: String,
        image: String,
        background: bool,
    }

    pub async fn push(file_contents: &Vec<u8>) -> Result<bool> {
        let base64_string = general_purpose::STANDARD.encode(file_contents);
        let device_id = dotenvy::var("TIDBYT_ID").expect("Missing TIDBYT_ID");
        let tidbyt_key = dotenvy::var("TIDBYT_KEY").expect("Missing TIDBYT_KEY");
        let endpoint = format!("https://api.tidbyt.com/v0/devices/{}/push", device_id);

        let payload = TidbytPayload {
            device_id,
            image: base64_string.clone(),
            installation_id: "custom".into(),
            background: true,
        };

        let resp = reqwest::Client::new()
            .post(&endpoint)
            .bearer_auth(tidbyt_key)
            .json(&payload)
            .header(USER_AGENT, "tidbyt")
            .send()
            .await?;

        if resp.status().as_u16() != 200 {
            println!("{:?}", resp.text().await.unwrap());
        } else {
            println!("Successfully pushed to Tidbyt");
        }

        Ok(true)
    }
}
