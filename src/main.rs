use std::io::Error;
use std::{collections::HashMap};
use std::env;
use std::net::Ipv4Addr;
use chrono::{Utc};
use image::{DynamicImage, GenericImageView, Pixel, GenericImage};
use mongodb::Collection;
use mongodb::options::FindOneOptions;
use warp::hyper::Body;
use warp::{http::Response, Filter};
use mongodb::{Client, options::ClientOptions};
use serde::{Deserialize, Serialize};
use mongodb::{bson::doc};
use rand::Rng;
use image::io::Reader as ImageReader;

#[tokio::main]
async fn main() {
    let check = warp::get()
        .and(warp::path("api"))
        .and(warp::path("check"))
        .and(warp::query::<HashMap<String, String>>())
        .and(warp::path::end())  
        .and_then(|p: HashMap<String, String>| async move {
            let id = p.get("id").unwrap();
            print!("entered");
            let challenge = find_challenge(String::from(id)).await;
            match challenge {
                None => Err(warp::reject::not_found()),
                Some(challenge) => Ok(ChallengeCheckDto::new(
                    challenge._id, 
                    challenge.actual.unwrap() == challenge.expected, 
                    challenge.timestamp.unwrap()))
            }
        });

    let answer = warp::post()
        .and(warp::path("api"))
        .and(warp::path("answer"))
        .and(warp::query::<HashMap<String, String>>())
        .and_then(|p: HashMap<String, String>| async move {
            let guid = p.get("id");
            let answer = p.get("answer");

            let res = match (guid, answer) {
                (Some(guid), Some(answer)) => {
                    let answer_int : u32 = answer.parse().unwrap();
                    update_challenge(String::from(guid), answer_int).await;
                    let challenge = find_challenge(String::from(guid)).await;

                    match challenge {
                        None => Err(warp::reject::not_found()),
                        Some(val) => Ok(ChallengeCheckDto::new(val._id, val.actual.unwrap() == val.expected, val.timestamp.unwrap()))
                    }
                }
                _ => Err(warp::reject()),
            };

            res
        });

        let start = warp::post()
            .and(warp::path("api"))
            .and(warp::path("start"))
            .and(warp::path::end())
            .and_then(|| async move {

                let image = get_image().await;
                let res = insert_challenge(image.0).await;
                
                match res{
                    Some(challenge) => Ok(ChallengeStartDto::new(challenge, image.1, image.2)),
                    None => Err(warp::reject()),
                }

            });


    let routes = check.or(answer).or(start);

    let port_key = "FUNCTIONS_CUSTOMHANDLER_PORT";
    let port: u16 = match env::var(port_key) {
        Ok(val) => val.parse().expect("Custom Handler port is not a number!"),
        Err(_) => 3000,
    };

    warp::serve(routes).run((Ipv4Addr::LOCALHOST, port)).await
}

async fn get_mongo_connection()-> Result<Client, mongodb::error::Error>{
    let conn_key = "ConnectionString_MongoDb";
    let connection_string : String = match env::var(conn_key) {
        Ok(val) => val,
        Err(_) => panic!("Failed reading connection string"),
    };
    // Parse a connection string into an options struct.
    let client_options = ClientOptions::parse(connection_string).await?;

    // Get a handle to the deployment.
    let client = Client::with_options(client_options)?;
    Ok(client)
}

#[derive(Debug, Serialize, Deserialize)]
struct Challenge{
    _id : mongodb::bson::oid::ObjectId,
    actual: Option<u32>,
    expected: u32,
    timestamp : Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChallengeInsert{
    _id : mongodb::bson::oid::ObjectId,
    expected: u32,
}

impl ChallengeInsert {
    fn new(expected: u32) -> Self{Self {_id: mongodb::bson::oid::ObjectId::new(), expected : expected}}
}


#[derive(Debug, Serialize, Deserialize)]
struct ChallengeStartDto{
    id : String,
    big_img: String,
    small_imgs: Vec<String>
}

impl ChallengeStartDto {
    fn new(id: String, big_img: String, small_imgs: Vec<String>) -> Self { Self { id,  big_img, small_imgs } }
}

impl warp::Reply for ChallengeStartDto{
    fn into_response(self) -> warp::reply::Response {
        let json = serde_json::to_string(&self).unwrap();

        Response::builder()
            .status(200)
            .header("Content-Type", "text/json")
            .body(Body::from(json))
            .unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChallengeCheckDto{
    id : String,
    success: bool,
    timestamp : i64,
    
}

impl ChallengeCheckDto {
    fn new(id: mongodb::bson::oid::ObjectId, success: bool, timestamp: i64) -> Self { Self { id: id.to_hex(), success, timestamp } }
}

impl warp::Reply for ChallengeCheckDto{
    fn into_response(self) -> warp::reply::Response {
        let json = serde_json::to_string(&self).unwrap();

        Response::builder()
            .status(200)
            .header("Content-Type", "text/json")
            .body(Body::from(json))
            .unwrap()
    }
}

async fn get_collection()-> Collection<Challenge>{
    let connection  = get_mongo_connection().await;
    let db = match connection{
        Ok(conn) => conn.database("CyberDb"),
        Err(_) => panic!("Couldn't connect to Mongo")
    };
    db.collection::<Challenge>("challenges")
}

async fn insert_collection()-> Collection<ChallengeInsert>{
    let connection  = get_mongo_connection().await;
    let db = match connection{
        Ok(conn) => conn.database("CyberDb"),
        Err(_) => panic!("Couldn't connect to Mongo")
    };
    db.collection::<ChallengeInsert>("challenges")
}

async fn find_challenge(guid : String) -> Option<Challenge> {
    let collection = get_collection().await;

    let filter = doc!{"_id" : mongodb::bson::oid::ObjectId::parse_str(guid).unwrap()};
    let options = FindOneOptions::builder().allow_partial_results(true).build();

    match collection.find_one(filter, options).await{
        Ok(val) => val,
        Err(err) => panic!("{}",err)
    }
}

async fn update_challenge(guid : String, answer: u32) {
    let collection = get_collection().await;

    let now = Utc::now().timestamp();
    let filter = doc!{"_id" : mongodb::bson::oid::ObjectId::parse_str(&guid).unwrap()};
    let update = doc!{"$set":{"actual": answer, "timestamp": now}};

    let result = collection.update_one(filter, update, None).await;
    match result {
        Ok(_) => (),
        Err(err) => panic!("{}",err)
    }
}

async fn insert_challenge(expected : u32) -> Option<String>{
    let collection = insert_collection().await;
    let challenge = ChallengeInsert::new(expected);
    let id = String::from(&challenge._id.to_string());
    let res = collection.insert_one(challenge, None).await;

    match res {
        Ok(_) => Some(id),
        Err(_) => panic!("Failed while inserting challenge")
    }
}

struct Img{
    pub image: DynamicImage,
    pub x: u32,
    pub y: u32
}

impl Img {
    fn new(image: DynamicImage, x: u32, y: u32) -> Self { Self { image, x, y } }
}

async fn get_image() -> (u32, String,  Vec<String>) {
    let path = get_random_image_name();
    let mut vec:Vec<Img> = Vec::new();
    let mut img = ImageReader::open(path).unwrap().decode().unwrap();
    for x in 0..4  {
        for y in 0..4{
            let z = img.crop_imm(x*128, y*128, 128, 128);
            let img = Img::new(z, x,y);
            vec.push(img);
        }
    }

    let index = rand::thread_rng().gen_range(0..vec.len());
    let selected = &vec[index];
    for x in 0..128{
        for y in 0..128 {
            let black : u8 = 0;
            let pixel = image::Rgba([black,black,black,255]);
            img.put_pixel(x+128*selected.x, y+128*selected.y, pixel)
        }
    }

    let mut k: Vec<String> = Vec::new();

    for img in vec {
        let b = img.image;
        let z = b.into_bytes();
        k.push(base64::encode(z));
    }
    
    let result_image = img.into_bytes();
    (index.try_into().unwrap(), base64::encode(&result_image), k)
}

fn get_random_image_name() -> String{
    let names = vec!["17-norway-landscape-photography.jpg","880-winter-rocky-landscape.jpg"];
    let length = names.len();
    let index: usize = rand::thread_rng().gen_range(0..length);

    String::from(names[index])
}