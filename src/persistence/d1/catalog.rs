use worker::D1Type::Text;

use crate::domain::{Game, Player, RosterStatus};
use crate::persistence::{GameRepository, PersistenceResult, RosterRepository};

use super::{
    records::{corrupt, GameRow, PlayerRow},
    sql, D1Repository,
};

impl RosterRepository for D1Repository {
    async fn add_player(&self, player: &Player) -> PersistenceResult<()> {
        let status = match player.status() {
            RosterStatus::Active => "active",
            RosterStatus::Inactive => "inactive",
        };
        self.insert_identity(
            sql::ADD_PLAYER,
            &[
                Text(player.id().as_str()),
                Text(player.name().as_str()),
                Text(status),
            ],
        )
        .await
    }

    async fn players(&self) -> PersistenceResult<Vec<Player>> {
        self.rows::<PlayerRow>(sql::PLAYERS, &[])
            .await?
            .into_iter()
            .map(|row| row.decode().map_err(corrupt))
            .collect()
    }
}

impl GameRepository for D1Repository {
    async fn add_game(&self, game: &Game) -> PersistenceResult<()> {
        self.insert_identity(
            sql::ADD_GAME,
            &[Text(game.id().as_str()), Text(game.name().as_str())],
        )
        .await
    }

    async fn games(&self) -> PersistenceResult<Vec<Game>> {
        self.rows::<GameRow>(sql::GAMES, &[])
            .await?
            .into_iter()
            .map(|row| row.decode().map_err(corrupt))
            .collect()
    }
}
