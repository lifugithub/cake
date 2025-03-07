use std::io::Write;

use crate::models::{chat::Message, Generator};

use super::{api, Context};

use anyhow::Result;

/// A master connects to, communicates with and orchestrates the workers.
pub struct Master<G> {
    pub ctx: Context,
    pub model: Box<G>,
}

impl<G: Generator + Send + Sync + 'static> Master<G> {
    /// Create a new instance.
    pub async fn new(ctx: Context) -> Result<Self> {
        let model = G::load(ctx.clone()).await?;
        Ok(Self { ctx, model })
    }

    pub async fn run(mut self) -> Result<()> {
        if self.ctx.args.api.is_some() {
            // run as REST api
            api::start(self).await?;
        } else {
            // if running in cli mode, pre add system and user prompts
            self.model
                .add_message(Message::system(self.ctx.args.system_prompt.clone()))?;
            self.model
                .add_message(Message::user(self.ctx.args.prompt.clone()))?;

            // just run one generation to stdout
            self.generate(|data| {
                if data.is_empty() {
                    println!();
                } else {
                    print!("{data}")
                }
                std::io::stdout().flush().unwrap();
            })
            .await?;
        }

        Ok(())
    }

    /// Reset the master state for a new inference.
    pub fn reset(&mut self) -> Result<()> {
        self.model.reset()
    }

    /// Start the generation loop and call the stream function for every token.
    pub async fn generate<S>(&mut self, mut stream: S) -> Result<()>
    where
        S: FnMut(&str),
    {
        log::info!(
            "starting the inference loop (mem={})\n\n",
            human_bytes::human_bytes(memory_stats::memory_stats().unwrap().physical_mem as f64)
        );

        log::debug!("  ctx.args.sample_len = {}", self.ctx.args.sample_len);

        stream(&self.ctx.args.prompt);

        let mut start_gen = std::time::Instant::now();

        for index in 0..self.ctx.args.sample_len {
            if index == 1 {
                // record start time again since the first token is the warmup
                start_gen = std::time::Instant::now()
            }

            let token = self.model.next_token(index).await?;
            if token.is_end_of_stream {
                break;
            } else {
                stream(&token.to_string());
            }
        }

        // signal end of stream
        stream("");

        let dt = start_gen.elapsed();
        let generated = self.model.generated_tokens();

        log::info!(
            "{} tokens generated ({} token/s) - mem={}",
            generated,
            (generated - 1) as f64 / dt.as_secs_f64(),
            human_bytes::human_bytes(memory_stats::memory_stats().unwrap().physical_mem as f64)
        );

        Ok(())
    }
}
