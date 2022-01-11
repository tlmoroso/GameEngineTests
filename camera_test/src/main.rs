use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_subscriber::{Registry, EnvFilter};
use tracing_appender::non_blocking;
use tracing_subscriber::layer::SubscriberExt;
use game_engine::game_loop::{GameLoop, GameLoopError};
use game_engine::input::multi_input::MultiInput;
use std::fmt::{Debug, Formatter};
use game_engine::scenes::{SceneLoader, SCENES_DIR, Scene};
use game_engine::load::{JSONLoad, LOAD_PATH, JSON_FILE, load_deserializable_from_file, create_entity_vec, load_deserializable_from_json};
use anyhow::{Result, Error};
use game_engine::game::GameWrapper;
use specs::{World, WorldExt, WriteStorage, Join, ReadStorage};
use game_engine::graphics::texture::{TextureHandle, TextureLoader, TEXTURE_LOAD_ID};
use game_engine::graphics::transform::{Transform, TransformLoader, TRANSFORM_LOAD_ID};
use game_engine::loading::{DrawTask, Task, GenTask};
use game_engine::scenes::scene_stack::{SceneStack, SceneStackLoader, SceneTransition, SCENE_STACK_FILE_ID};
use game_engine::globals::texture_dict::{TextureDictLoader, TEXTURE_DICT_LOAD_ID};
use game_engine::camera::orthographic_camera::{OrthographicCameraLoader, ORTHOGRAPHIC_CAMERA_LOAD_ID};
use game_engine::camera::Camera;
use game_engine::graphics::render::sprite_renderer::{SpriteRenderer, SpriteRenderError, SpriteRendererLoader};
use luminance_glfw::GL33Context;
use luminance_front::context::GraphicsContext;
use luminance_front::texture::Dim2;
use luminance_front::pipeline::PipelineState;
use game_engine::graphics::render::Renderer;
use glam::{Mat4, Vec3};
use game_engine::components::{ComponentMux, ComponentLoader};
use std::sync::{Arc, RwLock};
use serde::Deserialize;
use glfw::Key;
use serde_json::from_value;
use game_engine::graphics::Context;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Release, Acquire, Relaxed};

const CAMERA_TEST_ID: &str = "camera_test";
const CAMERA_TEST_SCENE_ID: &str = "camera_test_scene";

fn main() -> Result<(), GameLoopError> {
    let app_name = concat!(env!("CARGO_PKG_NAME"), "-", env!("CARGO_PKG_VERSION")).to_string();
    let file_appender = tracing_appender::rolling::never("C:/Users/tlmor/game_engine_tests/camera_test/", "camera_test.log");
    let (non_blocking_writer, _guard) = non_blocking(file_appender);

    let bunyan_formatting_layer = BunyanFormattingLayer::new(app_name, non_blocking_writer);
    let subscriber = Registry::default()
        .with(EnvFilter::from_default_env())
        .with(JsonStorageLayer)
        .with(bunyan_formatting_layer);

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set global default subscriber");

    let game_loop: GameLoop<TestGameWrapper, MultiInput> = GameLoop::new();
    game_loop.run(CAMERA_TEST_ID.to_string())
}

struct TestGameWrapper;

impl TestGameWrapper {
    fn scene_factory(json: JSONLoad) -> Result<Box<dyn SceneLoader<MultiInput>>> {
        match json.load_type_id.as_str() {
            CAMERA_TEST_SCENE_ID => Ok(Box::new(CameraTestSceneLoader::new(from_value(json.actual_value)?))),
            _ => {Err(Error::msg("Load ID did not match any scene ID"))}
        }
    }
}

impl GameWrapper<MultiInput> for TestGameWrapper {
    fn register_components(ecs: &mut World) {
        ecs.register::<TextureHandle>();
        ecs.register::<Transform>();
    }

    fn load() -> GenTask<SceneStack<MultiInput>> {
        let ss_loader = SceneStackLoader::new(
            [
                LOAD_PATH,
                CAMERA_TEST_ID,"/",
                SCENES_DIR,
                SCENE_STACK_FILE_ID,
                JSON_FILE
            ].join(""),
            TestGameWrapper::scene_factory
        );

        let td_loader = TextureDictLoader::new(
            [
                LOAD_PATH,
                CAMERA_TEST_ID,"/",
                TEXTURE_DICT_LOAD_ID,
                JSON_FILE
            ].join("")
        );

        let camera_loader = OrthographicCameraLoader::new(
            [
                LOAD_PATH,
                CAMERA_TEST_ID,"/",
                ORTHOGRAPHIC_CAMERA_LOAD_ID,
                JSON_FILE
            ].join("")
        );

        let td_task = td_loader.load()
            .map(|texture_dict, ecs| {
                ecs
                    .write()
                    .expect("Failed to lock World")
                    .insert(texture_dict);

                Ok(())
            });

        let camera_task = camera_loader.load()
            .map(|camera, ecs| {
                ecs.write()
                    .expect("Failed to acquire write lock for World")
                    .insert(Some(Box::new(camera) as Box<dyn Camera>));

                Ok(())
            });

        td_task.join(camera_task, |_| {})
            .sequence(ss_loader.load())
    }
}

pub struct CameraTestScene {
    sprite_renderer: RwLock<SpriteRenderer>,
    should_finish: AtomicBool
}

unsafe impl Send for CameraTestScene {}

unsafe impl Sync for CameraTestScene {}

impl Debug for CameraTestScene {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Camera Test Scene")
            .field("Sprite Renderer", &self.sprite_renderer.read()
                .expect("Failed to acquire read lock for renderer")
                .render_state
            )
            .finish()
    }
}

impl Scene<MultiInput> for CameraTestScene {
    fn update(&self, ecs: Arc<RwLock<World>>) -> Result<SceneTransition<MultiInput>> {
        let ecs = ecs.read().expect("Failed to acquire read lock for World.");
        let transforms: ReadStorage<Transform> = ecs.system_data();

        for transform in (&transforms).join() {
            transform.translation[0].store(transform.translation[0].load(Relaxed) + 1.0, Relaxed);
        }

        Ok(SceneTransition::NONE)
    }

    fn draw(&self, ecs: Arc<RwLock<World>>) -> Result<()> {
        let ecs = ecs.read().expect("Failed to acquire read lock for World");
        let context = ecs.fetch_mut::<Context>();

        let mut context = context.0
            .write()
            .expect("Failed to acquire write lock for Context");

        let back_buffer = context.back_buffer()
            .expect("Failed to get back buffer");

        context.new_pipeline_gate()
            .pipeline::<SpriteRenderError, Dim2, (), (), _>(
                &back_buffer,
                &PipelineState::default().set_clear_color([0.0, 0.0, 0.0, 1.0]),
                |pipeline, mut shading_gate| {
                    self.sprite_renderer.write()
                        .expect("Failed to acquire write lock for renderer")
                        .render(
                            &pipeline,
                            &mut shading_gate,
                            &Mat4::orthographic_rh_gl(
                                0.0,
                                960.0,
                                0.0,
                                540.0,
                                -1.0,
                                10.0
                            ),
                            ecs.deref()
                        ).unwrap();

                    Ok(())
                }
            );

        Ok(())
    }

    fn interact(&self, ecs: Arc<RwLock<World>>, input: &MultiInput) -> Result<()> {
        let ecs = ecs.read().expect("Failed to acquire read lock");

        let mut camera = ecs.fetch_mut::<Option<Box<dyn Camera>>>();

        if let Some(camera) = camera.deref_mut() {
            for key in input.get_pressed_keys() {
                match key.key {
                    Key::Left => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0] - 1.0, current_position[1], current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0] - 1.0, current_target[1], current_target[2]));
                    },
                    Key::Right => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0] + 1.0, current_position[1], current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0] + 1.0, current_target[1], current_target[2]));
                    },
                    Key::Up => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0], current_position[1] - 1.0, current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0], current_target[1] - 1.0, current_target[2]));
                    },
                    Key::Down => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0], current_position[1] + 1.0, current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0], current_target[1] + 1.0, current_target[2]));
                    },
                    _ => {}
                }
            }

            for key in input.get_held_keys() {
                match key.key {
                    Key::Left => {
                        println!("LEFT KEY HELD");
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0] - 3.0, current_position[1], current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0] - 3.0, current_target[1], current_target[2]));
                    },
                    Key::Right => {
                        println!("RIGHT KEY HELD");
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0] + 3.0, current_position[1], current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0] + 3.0, current_target[1], current_target[2]));
                    },
                    Key::Up => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0], current_position[1] - 3.0, current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0], current_target[1] - 3.0, current_target[2]));
                    },
                    Key::Down => {
                        let current_position = camera.position();
                        camera.set_position(Vec3::new(current_position[0], current_position[1] + 3.0, current_position[2]));
                        let current_target = camera.target();
                        camera.set_target(Vec3::new(current_target[0], current_target[1] + 3.0, current_target[2]));
                    },
                    Key::Q => self.should_finish.store(true, Release),
                    _ => {}
                }
            }
        } else {
            panic!("Camera does not exist.")
        }
        Ok(())
    }

    fn get_name(&self) -> String {
        String::from("Camera Test Scene")
    }

    fn is_finished(&self, _ecs: Arc<RwLock<World>>) -> Result<bool> {
        return Ok(self.should_finish.load(Acquire))
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct CameraTestSceneJSON {
    entity_paths: Vec<String>
}

#[derive(Debug)]
pub struct CameraTestSceneLoader {
    json: CameraTestSceneJSON,
}

impl CameraTestSceneLoader {
    pub fn new(json: CameraTestSceneJSON) -> Self {
        Self {
            json
        }
    }
}

impl ComponentMux for CameraTestSceneLoader {
    fn map_json_to_loader(json: JSONLoad) -> Result<Box<dyn ComponentLoader>> {
        match json.load_type_id.as_str() {
            TEXTURE_LOAD_ID => Ok(Box::new(TextureLoader::from_json(json)?)),
            TRANSFORM_LOAD_ID => Ok(Box::new(TransformLoader::from_json(json)?)),
            _ => Err(Error::msg("Invalid json load ID"))
        }
    }
}

impl SceneLoader<MultiInput> for CameraTestSceneLoader {
    fn load_scene(&self) -> GenTask<Box<dyn Scene<MultiInput>>> {
        let entity_paths = self.json.entity_paths.clone();
        SpriteRendererLoader::load_default()
            .serialize(
                Task::new(move |(renderer, ecs): (SpriteRenderer, Arc<RwLock<World>>)| {
                    create_entity_vec::<Self>(&entity_paths, ecs)?;
                    return Ok(renderer)
                })
            )
            .map(|renderer, _ecs| {
                Ok(Box::new(CameraTestScene {
                    sprite_renderer: RwLock::new(renderer),
                    should_finish: AtomicBool::new(false)
                }) as Box<dyn Scene<MultiInput>>)
            })
    }
}