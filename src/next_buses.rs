use crate::TextWidget;
use anyhow::{bail, Result};
use chrono::{DateTime, FixedOffset, Local};
use itertools::Itertools;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use reqwest::header::USER_AGENT;
use std::env;

pub async fn get_next_buses() -> Result<TextWidget> {
    let api_user = env::var("NEXT_BUSES_API_USER")?;
    let api_pass = env::var("NEXT_BUSES_API_PASS")?;
    let bus_stop_code = env::var("BUS_STOP_NAPTAN_CODE")?;
    let now: DateTime<FixedOffset> = Local::now().into();
    let now_str = now.to_rfc3339();
    let payload = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Siri version="1.0" xmlns="http://www.siri.org.uk/">
<ServiceRequest>
<RequestTimestamp>{}</RequestTimestamp>
<RequestorRef>{}</RequestorRef>
<StopMonitoringRequest version="1.0">
<RequestTimestamp>{}</RequestTimestamp>
<MessageIdentifier>{}</MessageIdentifier>
<MonitoringRef>{}</MonitoringRef>
</StopMonitoringRequest>
</ServiceRequest>
</Siri>"#,
        now_str, api_user, now_str, "garbage", bus_stop_code
    );
    let lookup = BusArrivalsLookup::from_xml(
        reqwest::Client::new()
            .post(format!(
                "http://{}:{}@nextbus.mxdata.co.uk/nextbuses/1/0/1",
                api_user, api_pass
            ))
            .body(payload)
            .header(USER_AGENT, "tidbyt")
            .send()
            .await?
            .text()
            .await?
            .as_str(),
    )?;
    Ok(TextWidget {
        text: lookup
            .arrivals
            .iter()
            .map(|arrival| {
                format!(
                    "{}, {} min",
                    arrival.line,
                    (arrival.expected_time - now).num_minutes()
                )
            })
            .join("\n"),
        color: String::from("#fff"),
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExpectedBusArrival {
    line: String,
    expected_time: DateTime<FixedOffset>,
}

impl ExpectedBusArrival {
    pub fn new_from_element(
        reader: &mut Reader<&[u8]>,
        _element: BytesStart,
        end_element: &[u8],
    ) -> Result<Self, anyhow::Error> {
        let mut buf = Vec::new();
        let mut line: Option<String> = None;
        let mut expected_time: Option<DateTime<FixedOffset>> = None;

        loop {
            let event = reader.read_event_into(&mut buf)?;

            match event {
                Event::Start(el) => match el.name().as_ref() {
                    b"PublishedLineName" => {
                        line = Some(reader.read_text(el.name())?.into());
                    }
                    b"ExpectedDepartureTime" => {
                        expected_time = Some(DateTime::parse_from_rfc3339(
                            reader.read_text(el.name())?.as_ref(),
                        )?)
                    }
                    _ => (),
                },
                Event::End(el) if el.name().as_ref() == end_element => break,
                Event::Eof => break,
                _ => (),
            }
        }

        let (line, expected_time) = if line.is_some() && expected_time.is_some() {
            (line.unwrap(), expected_time.unwrap())
        } else {
            bail!("did not parse");
        };

        Ok(ExpectedBusArrival {
            line,
            expected_time,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct BusArrivalsLookup {
    arrivals: Vec<ExpectedBusArrival>,
}

impl BusArrivalsLookup {
    pub fn from_xml(xml: &str) -> Result<BusArrivalsLookup, anyhow::Error> {
        let mut arrivals: Vec<ExpectedBusArrival> = vec![];

        let mut reader = Reader::from_str(xml);
        reader.trim_text(true);

        let mut buf = Vec::new();

        loop {
            let event = reader.read_event_into(&mut buf)?;

            match event {
                Event::Start(element) => match element.name().as_ref() {
                    b"MonitoredStopVisit" if arrivals.len() < 3 => {
                        arrivals.push(ExpectedBusArrival::new_from_element(
                            &mut reader,
                            element,
                            b"MonitoredStopVisit",
                        )?)
                    }
                    b"MonitoredStopVisit" => break,
                    _ => (),
                },
                Event::Eof => break,
                _ => (),
            }
        }

        Ok(BusArrivalsLookup { arrivals })
    }

    pub fn arrivals(&self) -> &[ExpectedBusArrival] {
        &self.arrivals
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    pub fn parse_response_to_lookup(xml_response: &str) {
        let expected_lookup = BusArrivalsLookup {
            arrivals: vec![
                ExpectedBusArrival {
                    line: "17".to_string(),
                    expected_time: DateTime::parse_from_rfc3339("2024-03-09T15:26:18.000Z")
                        .unwrap(),
                },
                ExpectedBusArrival {
                    line: "61".to_string(),
                    expected_time: DateTime::parse_from_rfc3339("2024-03-09T15:35:44.000Z")
                        .unwrap(),
                },
                ExpectedBusArrival {
                    line: "60A".to_string(),
                    expected_time: DateTime::parse_from_rfc3339("2024-03-09T15:35:59.000Z")
                        .unwrap(),
                },
            ],
        };
        assert_eq!(
            BusArrivalsLookup::from_xml(xml_response).unwrap(),
            expected_lookup
        );
    }

    #[fixture]
    pub fn xml_response() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Siri xmlns="http://www.siri.org.uk/" version="1.0">
    <ServiceDelivery>
        <ResponseTimestamp>2024-03-09T15:21:17.555Z</ResponseTimestamp>
        <StopMonitoringDelivery version="1.0">
            <ResponseTimestamp>2024-03-09T15:21:17.555Z</ResponseTimestamp>
            <RequestMessageRef>12345</RequestMessageRef>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>17</PublishedLineName>
                    <DirectionName>Central Station Union St</DirectionName>
                    <OperatorRef>WCMO</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:15:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:26:18.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>61</PublishedLineName>
                    <DirectionName>Sandyhills Balbeggie St</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:30:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:35:44.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60A</PublishedLineName>
                    <DirectionName>Easterhouse Terminus</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:23:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:35:59.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60</PublishedLineName>
                    <DirectionName>Easterhouse Terminus</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:38:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:38:19.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>X10</PublishedLineName>
                    <DirectionName>Glasgow Buchanan Bus Station</DirectionName>
                    <OperatorRef>MBLB</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:46:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:47:43.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60A</PublishedLineName>
                    <DirectionName>Easterhouse Terminus</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:53:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:53:00.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>17</PublishedLineName>
                    <DirectionName>Central Station Union St</DirectionName>
                    <OperatorRef>WCMO</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T15:55:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T15:55:06.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60</PublishedLineName>
                    <DirectionName>Easterhouse Terminus</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:08:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T16:08:00.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>61</PublishedLineName>
                    <DirectionName>Sandyhills Balbeggie St</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:00:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T16:14:34.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>17</PublishedLineName>
                    <DirectionName>Central Station Union St</DirectionName>
                    <OperatorRef>WCMO</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:15:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T16:15:00.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>61</PublishedLineName>
                    <DirectionName>Sandyhills Balbeggie St</DirectionName>
                    <OperatorRef>FG</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:15:00.000Z</AimedDepartureTime>
                        <ExpectedDepartureTime>2024-03-09T16:16:55.000Z</ExpectedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60A</PublishedLineName>
                    <DirectionName>Easterhouse, Lochdochart Road Terminus (unmarked)</DirectionName>
                    <OperatorRef>_noc_FGLA</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:23:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>61</PublishedLineName>
                    <DirectionName>Shettleston, Loch Laidon Street</DirectionName>
                    <OperatorRef>_noc_FGLA</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:30:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>17</PublishedLineName>
                    <DirectionName>Glasgow, Central Station</DirectionName>
                    <OperatorRef>_noc_GCTB</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:35:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60</PublishedLineName>
                    <DirectionName>Easterhouse, Lochdochart Road Terminus (unmarked)</DirectionName>
                    <OperatorRef>_noc_FGLA</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:38:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>61</PublishedLineName>
                    <DirectionName>Shettleston, Loch Laidon Street</DirectionName>
                    <OperatorRef>_noc_FGLA</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:45:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>X10A</PublishedLineName>
                    <DirectionName>Glasgow, Buchanan Bus Station</DirectionName>
                    <OperatorRef>_noc_MBLB</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:47:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>60A</PublishedLineName>
                    <DirectionName>Easterhouse, Lochdochart Road Terminus (unmarked)</DirectionName>
                    <OperatorRef>_noc_FGLA</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:53:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
            <MonitoredStopVisit>
                <RecordedAtTime>2024-03-09T15:21:17.555Z</RecordedAtTime>
                <MonitoringRef>45242629</MonitoringRef>
                <MonitoredVehicleJourney>
                    <FramedVehicleJourneyRef>
                        <DataFrameRef>-</DataFrameRef>
                        <DatedVehicleJourneyRef>-</DatedVehicleJourneyRef>
                    </FramedVehicleJourneyRef>
                    <VehicleMode>bus</VehicleMode>
                    <PublishedLineName>17</PublishedLineName>
                    <DirectionName>Glasgow, Central Station</DirectionName>
                    <OperatorRef>_noc_GCTB</OperatorRef>
                    <MonitoredCall>
                        <AimedDepartureTime>2024-03-09T16:55:00.000Z</AimedDepartureTime>
                    </MonitoredCall>
                </MonitoredVehicleJourney>
            </MonitoredStopVisit>
        </StopMonitoringDelivery>
    </ServiceDelivery>
</Siri>
        "#
    }
}
